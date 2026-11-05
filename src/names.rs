//! Process-wide numbering for new and restored unsaved documents.

use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, ensure};

pub const DEFAULT_PREFIX: &str = "Untitled";
static NEXT_NUMBER: AtomicU64 = AtomicU64::new(1);

/// Returns the next numbered title, shared by every tab and window in this process.
pub fn next_name(prefix: &str) -> String {
    next_name_from(&NEXT_NUMBER, prefix)
}

fn next_name_from(counter: &AtomicU64, prefix: &str) -> String {
    let number = counter.fetch_add(1, Ordering::Relaxed);
    format!("{} {number}", prefix.trim())
}

/// Reserves a restored title's trailing number so new tabs continue after it.
pub fn reserve_name(name: &str) {
    reserve_name_in(&NEXT_NUMBER, name);
}

fn reserve_name_in(counter: &AtomicU64, name: &str) {
    let name = name.trim_end();
    let start = name
        .trim_end_matches(|character: char| character.is_ascii_digit())
        .len();
    if let Some(next) = name[start..]
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_add(1))
    {
        counter.fetch_max(next, Ordering::Relaxed);
    }
}

/// Validates a document-name prefix that will also be offered in Save As.
pub fn validate_prefix(prefix: &str) -> Result<()> {
    ensure!(
        !prefix.chars().any(char::is_control),
        "The new document name prefix cannot contain control characters."
    );
    let prefix = prefix.trim();
    ensure!(!prefix.is_empty(), "Enter a name prefix for new documents.");
    ensure!(
        prefix.chars().count() <= 80,
        "The new document name prefix must contain at most 80 characters."
    );
    ensure!(
        !prefix.chars().any(|character| character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )),
        "The new document name prefix cannot contain control characters or < > : \" / \\ | ? *."
    );
    // A device name before a dot remains reserved even with the number after its extension.
    let numbered = format!("{prefix} 1");
    let stem = numbered
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    ensure!(
        !matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) && !["COM", "LPT"].iter().any(|device| {
            stem.strip_prefix(device).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }),
        "Choose a name prefix that does not create a reserved Windows file name."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tabs_share_a_monotonic_number_across_prefix_changes() {
        let counter = AtomicU64::new(1);
        assert_eq!(next_name_from(&counter, DEFAULT_PREFIX), "Untitled 1");
        assert_eq!(next_name_from(&counter, DEFAULT_PREFIX), "Untitled 2");
        assert_eq!(next_name_from(&counter, " Notes "), "Notes 3");
    }

    #[test]
    fn restored_names_reserve_the_highest_suffix_without_moving_backwards() {
        let counter = AtomicU64::new(1);
        reserve_name_in(&counter, "Untitled 12");
        reserve_name_in(&counter, "Notes 3");
        reserve_name_in(&counter, "Draft α17");
        reserve_name_in(&counter, "A title without a number");
        assert_eq!(next_name_from(&counter, "Notes"), "Notes 18");
    }

    #[test]
    fn malformed_or_overflowing_suffixes_do_not_reset_numbering() {
        let counter = AtomicU64::new(8);
        for title in [
            "",
            "Draft -",
            "Draft 18446744073709551615",
            "Draft 999999999999999999999999999999",
        ] {
            reserve_name_in(&counter, title);
        }
        assert_eq!(next_name_from(&counter, "Notes"), "Notes 8");
    }

    #[test]
    fn prefixes_allow_unicode_but_reject_invalid_save_as_names() {
        for prefix in ["Untitled", " Notes ", "α notes 🦀", "CON"] {
            assert!(validate_prefix(prefix).is_ok(), "valid prefix {prefix:?}");
        }
        assert!(validate_prefix(&"🦀".repeat(80)).is_ok());
        assert!(validate_prefix(&"a".repeat(81)).is_err());
        for prefix in [
            "",
            " \t",
            "Notes\nNext",
            "Notes\n",
            "\tNotes",
            "folder/name",
            "folder\\name",
            "A:B",
            "A?B",
            "A*B",
            "A\"B",
            "A<B",
            "A>B",
            "A|B",
            "CON.txt",
            "LPT1.notes",
        ] {
            assert!(
                validate_prefix(prefix).is_err(),
                "invalid prefix {prefix:?}"
            );
        }
    }
}
