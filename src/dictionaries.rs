//! Opt-in spelling dictionaries downloaded from pinned ExEditor repository assets.
use crate::proofing::SpellingBackend;
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

const MAX_BYTES: u64 = 512 * 1024 * 1024;
#[derive(Deserialize)]
pub struct Manifest {
    version: u32,
    source_commit: String,
    pub packs: Vec<DictionaryPack>,
}
#[derive(Deserialize)]
pub struct DictionaryPack {
    pub id: String,
    pub name: String,
    pub backend: SpellingBackend,
    pub language: String,
    pub license: String,
    pub source_url: String,
    pub pre_release: bool,
    primary: String,
    archive: Archive,
    files: Vec<Asset>,
}
#[derive(Deserialize)]
struct Archive {
    file: String,
    bytes: u64,
    sha256: String,
    parts: Vec<Part>,
}
#[derive(Deserialize)]
struct Part {
    file: String,
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct Asset {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize, Serialize)]
struct Receipt {
    archive_sha256: String,
    directory: String,
}
impl DictionaryPack {
    pub fn download_bytes(&self) -> u64 {
        self.archive.bytes
    }
}
pub fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let value: Manifest =
            serde_json::from_str(include_str!("../assets/accessories/dictionaries.json"))
                .expect("the compiled dictionary catalog must be valid");
        ensure_manifest(&value).expect("the compiled dictionary catalog must be safe");
        value
    })
}
fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\\', ':', '\0'])
        && Path::new(value)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && value.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.ends_with([' ', '.'])
        })
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn ensure_manifest(value: &Manifest) -> Result<()> {
    ensure!(
        value.version == 1
            && value.source_commit.len() == 40
            && value.source_commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid dictionary source pin"
    );
    let mut ids = HashSet::new();
    for pack in &value.packs {
        ensure!(
            pack.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
                && !pack.id.is_empty()
                && ids.insert(&pack.id),
            "Invalid or duplicate dictionary ID"
        );
        ensure!(
            pack.backend != SpellingBackend::System,
            "A download must use a dictionary engine"
        );
        ensure!(
            safe_relative(&pack.archive.file)
                && !pack.archive.file.contains('/')
                && pack.archive.bytes > 0
                && pack.archive.bytes <= MAX_BYTES
                && digest_valid(&pack.archive.sha256),
            "Invalid dictionary archive metadata"
        );
        ensure!(
            !pack.archive.parts.is_empty() && pack.archive.parts.len() <= 16,
            "Invalid dictionary archive parts"
        );
        let mut parts = HashSet::new();
        for part in &pack.archive.parts {
            ensure!(
                safe_relative(&part.file)
                    && !part.file.contains('/')
                    && parts.insert(&part.file)
                    && part.bytes > 0
                    && part.bytes <= 50 * 1024 * 1024
                    && digest_valid(&part.sha256),
                "Invalid dictionary archive part"
            );
        }
        ensure!(
            pack.archive.parts.iter().map(|p| p.bytes).sum::<u64>() == pack.archive.bytes,
            "Dictionary archive parts have the wrong total size"
        );
        ensure!(
            !pack.files.is_empty() && pack.files.len() <= 128,
            "Invalid dictionary file count"
        );
        let mut paths = HashSet::new();
        for file in &pack.files {
            ensure!(
                safe_relative(&file.path)
                    && paths.insert(file.path.to_lowercase())
                    && file.bytes <= MAX_BYTES
                    && digest_valid(&file.sha256),
                "Invalid dictionary file metadata"
            );
        }
        ensure!(
            pack.files.iter().map(|f| f.bytes).sum::<u64>() <= MAX_BYTES,
            "Dictionary expands beyond its limit"
        );
        ensure!(
            pack.primary == "."
                || (safe_relative(&pack.primary)
                    && pack.files.iter().any(|f| f.path == pack.primary)),
            "The dictionary entry point is absent"
        );
    }
    Ok(())
}
fn root() -> Result<PathBuf> {
    Ok(crate::document::data_dir()?.join("accessories/dictionaries"))
}
fn transfer(
    source: &mut impl Read,
    target: &mut impl Write,
    expected: u64,
    sha: &str,
) -> Result<()> {
    let mut limited = source.take(expected + 1);
    let mut digest = Sha256::new();
    let mut total = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let length = limited.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        total += length as u64;
        ensure!(
            total <= expected,
            "The dictionary file exceeds its recorded size. Retry the download."
        );
        digest.update(&buffer[..length]);
        target.write_all(&buffer[..length])?;
    }
    ensure!(
        total == expected && format!("{:x}", digest.finalize()) == sha,
        "Dictionary integrity check failed. Retry the download."
    );
    Ok(())
}
fn checked_file(root: &Path, relative: &str) -> Result<File> {
    ensure!(safe_relative(relative), "Invalid installed dictionary path");
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Dictionary files must not be symbolic links"
        );
    }
    let file = File::open(&path)?;
    ensure!(
        file.metadata()?.is_file(),
        "The dictionary file is unavailable"
    );
    Ok(file)
}
fn primary_path(pack: &DictionaryPack, directory: &Path) -> PathBuf {
    if pack.primary == "." {
        directory.to_path_buf()
    } else {
        directory.join(&pack.primary)
    }
}
fn installed_at(pack: &DictionaryPack, root: &Path) -> Result<Option<PathBuf>> {
    let base = root.join(&pack.id);
    if !base.try_exists()? {
        return Ok(None);
    }
    ensure!(
        !fs::symlink_metadata(&base)?.file_type().is_symlink(),
        "The dictionary folder must not be a symbolic link"
    );
    let receipt_path = base.join("installed.json");
    if !receipt_path.try_exists()? {
        return Ok(None);
    }
    let mut data = Vec::new();
    checked_file(&base, "installed.json")?
        .take(16 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= 16 * 1024,
        "The dictionary installation record is damaged"
    );
    let receipt: Receipt =
        serde_json::from_slice(&data).context("Reading the dictionary installation record")?;
    ensure!(
        receipt.archive_sha256 == pack.archive.sha256
            && safe_relative(&receipt.directory)
            && !receipt.directory.contains('/'),
        "The dictionary installation record does not match this version"
    );
    let directory = base.join(&receipt.directory);
    ensure!(
        !fs::symlink_metadata(&directory)?.file_type().is_symlink(),
        "The dictionary installation must not be a symbolic link"
    );
    for asset in &pack.files {
        let mut file = checked_file(&directory, &asset.path)?;
        ensure!(
            file.metadata()?.len() == asset.bytes,
            "The installed dictionary has the wrong size"
        );
        transfer(&mut file, &mut std::io::sink(), asset.bytes, &asset.sha256)?;
    }
    Ok(Some(primary_path(pack, &directory)))
}
/// Verifies existing data without using the network; call from a background worker.
pub fn installed_path(pack: &DictionaryPack) -> Result<Option<PathBuf>> {
    installed_at(pack, &root()?)
}
fn install_archive(pack: &DictionaryPack, archive: &Path, root: &Path) -> Result<PathBuf> {
    let mut source = File::open(archive)?;
    transfer(
        &mut source,
        &mut std::io::sink(),
        pack.archive.bytes,
        &pack.archive.sha256,
    )?;
    let base = root.join(&pack.id);
    fs::create_dir_all(&base).context("Creating the dictionary accessory folder")?;
    ensure!(
        !fs::symlink_metadata(&base)?.file_type().is_symlink(),
        "The dictionary folder must not be a symbolic link"
    );
    let staging = tempfile::Builder::new().prefix("pack-").tempdir_in(&base)?;
    let mut zip =
        zip::ZipArchive::new(File::open(archive)?).context("Opening the dictionary archive")?;
    ensure!(
        zip.len() == pack.files.len(),
        "The dictionary archive has unexpected files"
    );
    let mut seen = HashSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        ensure!(
            !entry.is_dir() && !entry.is_symlink() && safe_relative(entry.name()),
            "The dictionary archive contains an unsafe path"
        );
        let asset = pack
            .files
            .iter()
            .find(|asset| asset.path == entry.name())
            .context("Unexpected dictionary archive entry")?;
        ensure!(
            seen.insert(&asset.path) && entry.size() == asset.bytes,
            "The dictionary archive is incomplete or duplicated"
        );
        let path = staging.path().join(&asset.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = File::options().write(true).create_new(true).open(path)?;
        transfer(&mut entry, &mut target, asset.bytes, &asset.sha256)?;
        target.sync_all()?;
    }
    let receipt = Receipt {
        archive_sha256: pack.archive.sha256.clone(),
        directory: staging
            .path()
            .file_name()
            .context("Missing dictionary directory")?
            .to_string_lossy()
            .into_owned(),
    };
    // shortcut: retain older private versions while engines may hold them open; clean up after adding engine-aware removal.
    let directory = staging.keep();
    crate::document::atomic_write(&base.join("installed.json"), &serde_json::to_vec(&receipt)?)?;
    Ok(primary_path(pack, &directory))
}
/// Downloads only after a user action; streaming keeps large dictionaries out of memory.
pub fn download(pack: &'static DictionaryPack) -> Result<PathBuf> {
    let root = root()?;
    if let Ok(Some(path)) = installed_at(pack, &root) {
        return Ok(path);
    }
    fs::create_dir_all(&root)?;
    let mut archive = tempfile::NamedTempFile::new_in(&root)?;
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(600)))
            .max_redirects(0)
            .build(),
    );
    for part in &pack.archive.parts {
        let url = format!(
            "https://raw.githubusercontent.com/abelokoj/ExEditor/{}/assets/dictionaries/{}",
            manifest().source_commit,
            part.file
        );
        let mut response = agent.get(&url).call().with_context(|| {
            format!(
                "Downloading {}. Check your connection and retry.",
                pack.name
            )
        })?;
        ensure!(
            response.status().as_u16() == 200,
            "GitHub did not return the selected dictionary"
        );
        transfer(
            &mut response.body_mut().as_reader(),
            archive.as_file_mut(),
            part.bytes,
            &part.sha256,
        )?;
    }
    archive.as_file().sync_all()?;
    install_archive(pack, archive.path(), &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dictionary_catalog_and_repository_parts_match() {
        assert_eq!(manifest().packs.len(), 18);
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dictionaries");
        for pack in &manifest().packs {
            for part in &pack.archive.parts {
                let mut file = File::open(repository.join(&part.file)).unwrap();
                transfer(&mut file, &mut std::io::sink(), part.bytes, &part.sha256).unwrap();
            }
        }
    }
    #[test]
    fn dictionary_paths_reject_escape_and_windows_aliases() {
        for bad in [
            "../x",
            "/x",
            "C:/x",
            "file:stream",
            "a\\b",
            "./a",
            "a/../b",
            "a//b",
            "a.",
            "a ",
            "a/",
            "",
        ] {
            assert!(!safe_relative(bad), "{bad}");
        }
        assert!(safe_relative("5/mor-standard/mor.vfst"));
    }
    #[test]
    fn corrupt_repair_never_replaces_the_previous_verified_installation() {
        let pack = &manifest().packs[0];
        let root = tempfile::tempdir().unwrap();
        let archive = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/dictionaries")
            .join(&pack.archive.parts[0].file);
        assert!(installed_at(pack, root.path()).unwrap().is_none());
        let path = install_archive(pack, &archive, root.path()).unwrap();
        assert_eq!(installed_at(pack, root.path()).unwrap(), Some(path.clone()));
        let broken = root.path().join("broken.zip");
        fs::write(&broken, b"not a dictionary archive").unwrap();
        assert!(install_archive(pack, &broken, root.path()).is_err());
        assert_eq!(installed_at(pack, root.path()).unwrap(), Some(path.clone()));
        fs::write(&path, b"corrupted installed data").unwrap();
        assert!(installed_at(pack, root.path()).is_err());
        let repaired = install_archive(pack, &archive, root.path()).unwrap();
        assert_ne!(path, repaired);
        assert_eq!(installed_at(pack, root.path()).unwrap(), Some(repaired));
    }
    #[test]
    #[ignore = "network check executed explicitly by GitHub on Windows"]
    fn github_dictionary_downloads_are_verified_and_cached() {
        let pack = &manifest().packs[0];
        let root = tempfile::tempdir().unwrap();
        let mut archive = tempfile::NamedTempFile::new().unwrap();
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(120)))
                .max_redirects(0)
                .build(),
        );
        for part in &pack.archive.parts {
            let url = format!(
                "https://raw.githubusercontent.com/abelokoj/ExEditor/{}/assets/dictionaries/{}",
                manifest().source_commit,
                part.file
            );
            let mut response = agent.get(&url).call().unwrap();
            transfer(
                &mut response.body_mut().as_reader(),
                archive.as_file_mut(),
                part.bytes,
                &part.sha256,
            )
            .unwrap();
        }
        let path = install_archive(pack, archive.path(), root.path()).unwrap();
        assert_eq!(installed_at(pack, root.path()).unwrap(), Some(path));
    }
}
