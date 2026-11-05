//! TextEdit-compatible defaults and local application preferences.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::document::{TextEncoding, atomic_write, data_dir};

const RECENT_FILE_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub rich_text: bool,
    pub rich_font: String,
    pub rich_size: f32,
    pub plain_font: String,
    pub plain_size: f32,
    pub wrap_to_page: bool,
    pub show_ruler: bool,
    pub spellcheck: bool,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub automatic_links: bool,
    pub autosave_seconds: u64,
    pub appearance: Appearance,
    pub recent_files: Vec<PathBuf>,
    pub default_encoding: TextEncoding,
    pub remember_windows: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            rich_text: true,
            rich_font: crate::fonts::RICH_FONT.into(),
            rich_size: 12.0,
            plain_font: crate::fonts::PLAIN_FONT.into(),
            plain_size: 11.0,
            wrap_to_page: false,
            show_ruler: true,
            spellcheck: true,
            smart_quotes: true,
            smart_dashes: true,
            automatic_links: true,
            autosave_seconds: 30,
            appearance: Appearance::System,
            recent_files: Vec::new(),
            default_encoding: TextEncoding::Utf8,
            remember_windows: true,
        }
    }
}

impl Preferences {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join("preferences.json");
        if !path.try_exists()? {
            return Ok(Self::default());
        }
        let bytes = fs::read(&path).context("Reading ExEditor preferences")?;
        ensure!(
            bytes.len() <= 1024 * 1024,
            "The preferences file is unexpectedly large; it has not been modified."
        );
        let preferences: Self = serde_json::from_slice(&bytes).context(
            "Reading ExEditor preferences; the original settings file has not been modified",
        )?;
        preferences.validate()?;
        Ok(preferences)
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        atomic_write(&data_dir()?.join("preferences.json"), &bytes)
    }

    pub fn add_recent(&mut self, path: PathBuf) {
        let path = fs::canonicalize(&path).unwrap_or(path);
        self.recent_files.retain(|existing| {
            #[cfg(windows)]
            {
                !existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&path.to_string_lossy())
            }
            #[cfg(not(windows))]
            {
                existing != &path
            }
        });
        self.recent_files.insert(0, path);
        self.recent_files.truncate(RECENT_FILE_LIMIT);
    }

    pub fn clear_recent(&mut self) {
        self.recent_files.clear();
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            !self.rich_font.trim().is_empty() && self.rich_font.len() <= 256,
            "Choose a valid default rich-text font."
        );
        ensure!(
            !self.plain_font.trim().is_empty() && self.plain_font.len() <= 256,
            "Choose a valid default plain-text font."
        );
        ensure!(
            self.rich_size.is_finite() && (1.0..=1638.0).contains(&self.rich_size),
            "The rich-text font size must be between 1 and 1638 points."
        );
        ensure!(
            self.plain_size.is_finite() && (1.0..=1638.0).contains(&self.plain_size),
            "The plain-text font size must be between 1 and 1638 points."
        );
        ensure!(
            (5..=3600).contains(&self.autosave_seconds),
            "The recovery interval must be between 5 seconds and one hour."
        );
        ensure!(
            self.recent_files.len() <= RECENT_FILE_LIMIT,
            "The recent-document list exceeds its limit."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_preferences_inherit_new_defaults() {
        let preferences: Preferences =
            serde_json::from_str(r#"{"rich_text":false}"#).expect("older settings");
        assert!(!preferences.rich_text);
        assert!(preferences.spellcheck);
        assert_eq!(preferences.autosave_seconds, 30);
    }

    #[test]
    fn recents_deduplicate_and_remain_bounded() {
        let mut preferences = Preferences::default();
        for index in 0..30 {
            preferences.add_recent(PathBuf::from(format!("document-{index}.rtf")));
        }
        assert_eq!(preferences.recent_files.len(), RECENT_FILE_LIMIT);
        preferences.add_recent(PathBuf::from("document-25.rtf"));
        assert_eq!(
            preferences.recent_files[0],
            PathBuf::from("document-25.rtf")
        );
        assert_eq!(preferences.recent_files.len(), RECENT_FILE_LIMIT);
    }

    #[test]
    fn invalid_sizes_and_recovery_intervals_are_rejected() {
        let mut preferences = Preferences {
            rich_size: f32::NAN,
            ..Preferences::default()
        };
        assert!(preferences.validate().is_err());
        preferences.rich_size = 12.0;
        preferences.autosave_seconds = 0;
        assert!(preferences.validate().is_err());
    }
}
