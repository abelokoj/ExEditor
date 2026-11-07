//! Persistent unsaved buffers with one live owner per snapshot.

use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

use anyhow::{Context, Result};

use crate::document::{self, RecoveryDocument};

/// Holds a buffer's exclusive operating-system lease until its editor is dropped.
///
/// Closing this handle releases ownership without removing the lock or snapshot.
#[derive(Debug)]
pub struct BufferLease {
    _file: File,
}

#[derive(Debug)]
pub struct RestoredBuffer {
    pub document: RecoveryDocument,
    pub lease: BufferLease,
}

#[derive(Debug, Default)]
pub struct RestoreResult {
    pub buffers: Vec<RestoredBuffer>,
    pub warnings: Vec<String>,
}

/// Claims a buffer, or returns `None` when another live editor already owns it.
///
/// # Errors
/// Returns an error for an invalid identifier or a file-system failure other than
/// another process holding the Windows exclusive-sharing lease.
pub fn lease_buffer(id: &str) -> Result<Option<BufferLease>> {
    document::validate_recovery_id(id)?;
    lease_buffer_in(&document::recovery_dir()?, id)
}

fn lease_buffer_in(folder: &Path, id: &str) -> Result<Option<BufferLease>> {
    document::validate_recovery_id(id)?;
    let path = folder.join(format!("{id}.lock"));
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0);
    }
    #[cfg(not(windows))]
    anyhow::bail!("Unsaved-buffer ownership is currently supported on Windows only.");
    #[cfg(windows)]
    match options.open(&path) {
        Ok(file) => Ok(Some(BufferLease { _file: file })),
        // Windows ERROR_SHARING_VIOLATION means another live handle owns the lease.
        Err(error) if error.raw_os_error() == Some(32) => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!("Protecting unsaved buffer {id}; the existing snapshot has not been changed")
        }),
    }
}

/// Restores inactive buffers independently, preserving any damaged snapshots.
///
/// Ownership is acquired before reading a snapshot, so another ExEditor process
/// cannot restore the same buffer while the returned lease is retained.
pub fn restore_buffers() -> Result<RestoreResult> {
    restore_buffers_in(&document::recovery_dir()?)
}

fn restore_buffers_in(folder: &Path) -> Result<RestoreResult> {
    let entries = fs::read_dir(folder).context("Reading the unsaved-buffer folder")?;
    let mut result = RestoreResult::default();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                result.warnings.push(format!(
                    "One unsaved buffer could not be inspected: {error}. Its snapshot has been preserved."
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.extension() != Some(std::ffi::OsStr::new("json")) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(std::ffi::OsStr::to_str) else {
            result.warnings.push(format!(
                "An unsaved buffer has an unreadable file name: {}. Its snapshot has been preserved.",
                path.display()
            ));
            continue;
        };
        let lease = match lease_buffer_in(folder, id) {
            Ok(Some(lease)) => lease,
            Ok(None) => continue,
            Err(error) => {
                result.warnings.push(format!("{error:#}"));
                continue;
            }
        };
        match document::read_recovery(&path) {
            Ok(document) => result.buffers.push(RestoredBuffer { document, lease }),
            Err(error) => result.warnings.push(format!(
                "Could not restore {}: {error:#}. The snapshot has been preserved.",
                path.display()
            )),
        }
    }
    // Parsed RFC 3339 values sort by actual time even across daylight-saving offsets.
    result.buffers.sort_by(|left, right| {
        let left_time = chrono::DateTime::parse_from_rfc3339(&left.document.saved_at)
            .expect("read_recovery validates the snapshot timestamp");
        let right_time = chrono::DateTime::parse_from_rfc3339(&right.document.saved_at)
            .expect("read_recovery validates the snapshot timestamp");
        left_time
            .cmp(&right_time)
            .then_with(|| left.document.id.cmp(&right.document.id))
    });
    Ok(result)
}

/// Each tab permits one outstanding snapshot. Its lease lives until the write
/// completes; callers drain this receiver before saving, discarding or closing.
pub fn queue_recovery(
    snapshot: RecoveryDocument,
    lease: std::sync::Arc<BufferLease>,
) -> std::sync::mpsc::Receiver<Result<()>> {
    recovery_job(move || document::write_recovery(&snapshot), lease)
}
fn recovery_job(
    work: impl FnOnce() -> Result<()> + Send + 'static,
    lease: std::sync::Arc<BufferLease>,
) -> std::sync::mpsc::Receiver<Result<()>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    smol::spawn(async move {
        let result = smol::unblock(move || {
            let _lease = lease;
            work()
        })
        .await;
        let _ = sender.send(result);
    })
    .detach();
    receiver
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::document::{DocumentContent, DocumentFormat, LineEnding, TextEncoding};
    use tempfile::TempDir;

    #[test]
    fn background_write_holds_the_exclusive_lease_until_completion() {
        use std::{
            sync::{Arc, mpsc},
            time::Duration,
        };
        let folder = TempDir::new().expect("private buffer folder");
        let lease = Arc::new(
            lease_buffer_in(folder.path(), "pending")
                .expect("lease")
                .expect("unused"),
        );
        let (started, waiting) = mpsc::channel();
        let (release, finish) = mpsc::channel();
        let result = recovery_job(
            move || {
                started.send(()).context("starting write")?;
                finish.recv().context("finishing write")?;
                Ok(())
            },
            lease,
        );
        waiting
            .recv_timeout(Duration::from_secs(5))
            .expect("background write started");
        assert!(
            lease_buffer_in(folder.path(), "pending")
                .expect("check pending lease")
                .is_none()
        );
        release.send(()).expect("finish write");
        result
            .recv_timeout(Duration::from_secs(5))
            .expect("completion")
            .expect("write succeeded");
        assert!(
            lease_buffer_in(folder.path(), "pending")
                .expect("released lease")
                .is_some()
        );
    }

    fn snapshot(id: &str, text: &str, saved_at: &str) -> RecoveryDocument {
        RecoveryDocument {
            id: id.into(),
            original_path: None,
            format: DocumentFormat::PlainText,
            content: DocumentContent::Plain(text.into()),
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::Lf,
            saved_at: saved_at.into(),
            untitled_name: None,
            editor_snapshot: None,
        }
    }

    fn write_snapshot(folder: &Path, document: &RecoveryDocument) {
        let bytes = serde_json::to_vec(document).expect("serializing the buffer fixture");
        document::atomic_write(&folder.join(format!("{}.json", document.id)), &bytes)
            .expect("writing the buffer fixture");
    }

    #[test]
    fn exclusive_lease_releases_without_deleting_snapshot_or_lock() {
        let folder = TempDir::new().expect("temporary buffer folder");
        let buffer = snapshot("draft", "Never saved 🦀", "2026-11-05T10:00:00-05:00");
        write_snapshot(folder.path(), &buffer);
        let original =
            fs::read(folder.path().join("draft.json")).expect("reading snapshot fixture");
        let lease = lease_buffer_in(folder.path(), "draft")
            .expect("first lease")
            .expect("unowned buffer");
        assert!(
            lease_buffer_in(folder.path(), "draft")
                .expect("second lease attempt")
                .is_none(),
            "the existing live lease must prevent a second owner"
        );
        drop(lease);
        assert!(folder.path().join("draft.lock").is_file());
        assert_eq!(
            fs::read(folder.path().join("draft.json")).expect("retained snapshot"),
            original
        );
        assert!(
            lease_buffer_in(folder.path(), "draft")
                .expect("lease after closing the editor")
                .is_some()
        );
    }

    #[test]
    fn restoring_skips_a_buffer_owned_by_another_editor() {
        let folder = TempDir::new().expect("temporary buffer folder");
        write_snapshot(
            folder.path(),
            &snapshot("draft", "live edits", "2026-11-05T10:00:00-05:00"),
        );
        let lease = lease_buffer_in(folder.path(), "draft")
            .expect("claiming the live buffer")
            .expect("unowned buffer");
        let restored = restore_buffers_in(folder.path()).expect("reading buffer snapshots");
        assert!(restored.buffers.is_empty());
        assert!(restored.warnings.is_empty());
        drop(lease);
        let restored = restore_buffers_in(folder.path()).expect("reading the released snapshot");
        assert_eq!(restored.buffers.len(), 1);
        assert_eq!(
            restored.buffers[0].document.content,
            DocumentContent::Plain("live edits".into())
        );
        assert!(
            lease_buffer_in(folder.path(), "draft")
                .expect("checking retained restored ownership")
                .is_none()
        );
    }

    #[test]
    fn corrupt_snapshot_does_not_block_healthy_buffers_or_destroy_bytes() {
        let folder = TempDir::new().expect("temporary buffer folder");
        write_snapshot(
            folder.path(),
            &snapshot("healthy", "Recover me", "2026-11-05T10:00:00-05:00"),
        );
        let damaged_path = folder.path().join("damaged.json");
        let damaged = b"{truncated snapshot";
        fs::write(&damaged_path, damaged).expect("writing a damaged snapshot fixture");
        let restored = restore_buffers_in(folder.path()).expect("restoring healthy buffers");
        assert_eq!(restored.buffers.len(), 1);
        assert_eq!(restored.buffers[0].document.id, "healthy");
        assert_eq!(restored.warnings.len(), 1);
        assert_eq!(
            fs::read(damaged_path).expect("preserved damaged snapshot"),
            damaged
        );
    }

    #[test]
    fn invalid_ids_do_not_create_lock_files() {
        let folder = TempDir::new().expect("temporary buffer folder");
        for id in ["", "../draft", "..\\draft", "draft:stream", "draft/name"] {
            assert!(
                lease_buffer_in(folder.path(), id).is_err(),
                "identifier {id:?}"
            );
        }
        assert_eq!(
            fs::read_dir(folder.path())
                .expect("reading fixture folder")
                .count(),
            0
        );
    }

    #[test]
    fn snapshot_identity_mismatch_is_preserved_and_reported() {
        let folder = TempDir::new().expect("temporary buffer folder");
        let buffer = snapshot("different", "Private notes", "2026-11-05T10:00:00-05:00");
        let bytes = serde_json::to_vec(&buffer).expect("serializing fixture");
        let path = folder.path().join("draft.json");
        fs::write(&path, &bytes).expect("writing mismatched snapshot fixture");
        let restored = restore_buffers_in(folder.path()).expect("reading mismatched snapshots");
        assert!(restored.buffers.is_empty());
        assert_eq!(restored.warnings.len(), 1);
        assert_eq!(fs::read(path).expect("retained mismatched snapshot"), bytes);
    }

    #[test]
    fn only_sharing_conflicts_are_treated_as_another_owner() {
        let folder = TempDir::new().expect("temporary buffer folder");
        fs::create_dir(folder.path().join("draft.lock")).expect("creating an invalid lock fixture");
        assert!(lease_buffer_in(folder.path(), "draft").is_err());
    }

    #[test]
    fn restored_buffers_sort_chronologically_across_offsets() {
        let folder = TempDir::new().expect("temporary buffer folder");
        write_snapshot(
            folder.path(),
            &snapshot("later", "later", "2026-11-05T10:00:00-05:00"),
        );
        write_snapshot(
            folder.path(),
            &snapshot("earlier", "earlier", "2026-11-05T11:00:00-03:00"),
        );
        let restored = restore_buffers_in(folder.path()).expect("restoring timestamp fixtures");
        assert_eq!(restored.buffers[0].document.id, "earlier");
        assert_eq!(restored.buffers[1].document.id, "later");
    }
    #[test]
    fn restoring_a_buffer_keeps_its_numbered_untitled_name() {
        let folder = TempDir::new().expect("temporary buffer folder");
        let mut document = snapshot("draft", "Private notes", "2026-11-05T10:00:00-05:00");
        document.untitled_name = Some("Notes 12".into());
        write_snapshot(folder.path(), &document);
        let restored = restore_buffers_in(folder.path()).expect("restoring a named buffer");
        assert_eq!(restored.buffers.len(), 1);
        assert_eq!(
            restored.buffers[0].document.untitled_name.as_deref(),
            Some("Notes 12")
        );
    }
}
