//! Platform language services and local typographic editing helpers.

use std::ops::Range;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{Speech, check_spelling_for_language};

/// A spelling finding with offsets in the editor's UTF-16 coordinate system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellingIssue {
    /// The affected UTF-16 code units, excluding any terminating NUL.
    pub range: Range<usize>,
    /// The original document text in that range.
    pub word: String,
    /// Suggested replacements in the provider's preferred order; empty text means delete.
    pub suggestions: Vec<String>,
}

#[cfg(not(target_os = "windows"))]
pub fn check_spelling_for_language(
    _text: &str,
    _language: &str,
) -> anyhow::Result<Vec<SpellingIssue>> {
    anyhow::bail!(
        "The Windows spelling dictionary is unavailable on this platform. Choose an offline spelling provider in Language Tools."
    )
}

/// A cancellable utterance using an installed native speech command.
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Default)]
pub struct Speech {
    child: Option<std::process::Child>,
}

#[cfg(not(target_os = "windows"))]
impl Speech {
    /// Starts speech through macOS say or Linux eSpeak, if installed.
    pub fn start(&mut self, text: &str) -> anyhow::Result<()> {
        use anyhow::{Context as _, ensure};
        use std::{
            io::Write as _,
            process::{Command, Stdio},
        };
        ensure!(
            !text.contains('\0'),
            "speech input contains a NUL character"
        );
        self.stop()?;
        if text.is_empty() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        let commands: &[(&str, &[&str])] = &[("/usr/bin/say", &[])];
        #[cfg(not(target_os = "macos"))]
        let commands: &[(&str, &[&str])] = &[("espeak-ng", &["--stdin"]), ("espeak", &["--stdin"])];
        for (program, arguments) in commands {
            let child = Command::new(program)
                .args(*arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            let mut child = match child {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("starting native speech synthesis"),
            };
            let input = child
                .stdin
                .take()
                .context("opening speech input")?
                .write_all(text.as_bytes());
            if let Err(error) = input {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("sending text to speech synthesis");
            }
            self.child = Some(child);
            return Ok(());
        }
        anyhow::bail!(
            "Native speech synthesis is unavailable. Install eSpeak NG on Linux or enable the macOS say service."
        )
    }

    /// Stops only the utterance this editor started and reaps its process.
    pub fn stop(&mut self) -> anyhow::Result<()> {
        use anyhow::Context as _;
        if let Some(child) = self.child.as_mut()
            && child
                .try_wait()
                .context("checking speech synthesis")?
                .is_none()
        {
            child.kill().context("stopping speech synthesis")?;
            child.wait().context("finishing speech synthesis")?;
        }
        self.child = None;
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for Speech {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A local replacement with offsets in the editor's UTF-16 coordinate system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSubstitution {
    /// The exact code units to replace, leaving surrounding formatting intact.
    pub range: Range<usize>,
    /// The typographic quote or dash replacing the user's most recent insertion.
    pub replacement: String,
}

/// Suggests one smart quote or dash immediately before the supplied UTF-16 caret.
/// Returns no change for disabled substitutions or an invalid Unicode boundary.
pub fn substitution(
    text: &str,
    caret_utf16: usize,
    smart_quotes: bool,
    smart_dashes: bool,
) -> Option<TextSubstitution> {
    let caret_byte = utf16_byte_offset(text, caret_utf16)?;
    let preceding = &text[..caret_byte];
    if smart_dashes && preceding.ends_with("--") {
        return Some(TextSubstitution {
            range: caret_utf16 - 2..caret_utf16,
            replacement: "—".to_owned(),
        });
    }
    if !smart_quotes {
        return None;
    }
    let mut chars = preceding.chars().rev();
    let typed = chars.next()?;
    if typed != '\'' && typed != '"' {
        return None;
    }
    let previous = chars.next();
    let opening = previous.is_none_or(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '(' | '[' | '{' | '<' | '—' | '–' | ':' | ';' | '=' | '“' | '‘'
            )
    });
    let replacement = match (typed, opening) {
        ('\'', true) => "‘",
        ('\'', false) => "’",
        ('"', true) => "“",
        ('"', false) => "”",
        _ => return None,
    };
    Some(TextSubstitution {
        range: caret_utf16 - 1..caret_utf16,
        replacement: replacement.to_owned(),
    })
}

pub(crate) fn utf16_byte_offset(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::{TextSubstitution, substitution, utf16_byte_offset};

    #[test]
    fn smart_dashes_keep_utf16_offsets_after_emoji() {
        assert_eq!(
            substitution("😀--", 4, false, true),
            Some(TextSubstitution {
                range: 2..4,
                replacement: "—".to_owned()
            })
        );
    }

    #[test]
    fn smart_quotes_distinguish_opening_closing_and_apostrophes() {
        for (text, caret, expected) in [
            ("\"hello", 1, "“"),
            ("hello\"", 6, "”"),
            ("can\'t", 4, "’"),
            ("(\'hello", 2, "‘"),
            ("He said: \"here", 10, "“"),
        ] {
            assert_eq!(
                substitution(text, caret, true, false).map(|item| item.replacement),
                Some(expected.to_owned()),
                "input {text:?}"
            );
        }
    }

    #[test]
    fn substitutions_ignore_disabled_features_and_replacement_characters() {
        assert_eq!(substitution("--", 2, true, false), None);
        assert_eq!(substitution("\"", 1, false, true), None);
        assert_eq!(substitution("“", 1, true, true), None);
        assert_eq!(substitution("—", 1, true, true), None);
    }

    #[test]
    fn invalid_surrogate_offsets_are_rejected() {
        assert_eq!(utf16_byte_offset("😀\"", 1), None);
        assert_eq!(utf16_byte_offset("😀\"", 2), Some(4));
        assert_eq!(substitution("😀\"", 1, true, true), None);
        assert_eq!(substitution("😀\"", 4, true, true), None);
        assert_eq!(
            substitution("😀\"", 3, true, true).map(|item| item.range),
            Some(2..3)
        );
    }
}
