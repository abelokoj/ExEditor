//! Shared Windows editing shortcuts for native text and GPUI windows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

pub fn shortcut(key: u32, m: Modifiers, chord: bool) -> Option<&'static str> {
    if chord {
        return match (key, m.control, m.shift, m.alt) {
            (0x53, true, false, false) => Some("keyboard-help"),
            (0x53, false, false, false) => Some("save-all"),
            (0x43, true, false, false) => Some("add-comment"),
            (0x55, true, false, false) => Some("remove-comment"),
            (0x58, true, false, false) => Some("trim-whitespace"),
            (0x57, true, false, false) => Some("close-all"),
            (0x56, false, false, false) => Some("markdown-preview"),
            (0x50, false, false, false) => Some("copy-path"),
            (0x52, false, false, false) => Some("reveal-file"),
            _ => None,
        };
    }
    if !m.control && !m.alt {
        return match (key, m.shift) {
            (0x70, false) => Some("command-palette"), // F1
            (0x72, false) => Some("find-next"),
            (0x72, true) => Some("find-previous"),
            (0x7a, false) => Some("fullscreen"),
            _ => None,
        };
    }
    if m.alt && !m.control {
        return match (key, m.shift) {
            (0x26, false) => Some("move-line-up"),
            (0x28, false) => Some("move-line-down"),
            (0x26, true) => Some("copy-line-up"),
            (0x28, true) => Some("copy-line-down"),
            (0x5a, false) => Some("word-wrap"),
            (0x41, true) => Some("block-comment"),
            (0x21, false) => Some("scroll-page-up"),
            (0x22, false) => Some("scroll-page-down"),
            _ => None,
        };
    }
    if !m.control || m.alt {
        return None;
    } // AltGr remains available for text input.
    Some(match (key, m.shift) {
        (0x4b, false) => "chord",
        (0x4e, false) => "new-tab",
        (0x4e, true) => "new",
        (0x4f | 0x50, false) => "open",
        (0x50, true) => "command-palette",
        (0x53, false) => "save",
        (0x53, true) => "save-as",
        (0x57, false) | (0x73, false) => "close",
        (0x57, true) => "close-window",
        (0x46, false) => "find",
        (0x48, false) => "replace",
        (0x47, false) => "go-to-line",
        (0x4c, false) => "select-line",
        (0x0d, false) => "insert-line-below",
        (0x0d, true) => "insert-line-above",
        (0x4b, true) => "delete-line",
        (0xdb, false) => "outdent-line",
        (0xdd, false) => "indent-line",
        (0xbf, false) => "toggle-comment",
        (0xdc, true) => "matching-bracket",
        (0x26, false) => "scroll-line-up",
        (0x28, false) => "scroll-line-down",
        (0x43, false) => "copy",
        (0x58, false) => "cut",
        (0x56, false) => "paste",
        (0x56, true) => "paste-or-preview",
        (0x5a, false) => "undo",
        (0x5a, true) | (0x59, false) => "redo",
        (0x41, false) => "select-all",
        (0x42, false) => "bold",
        (0x49, false) => "italic",
        (0x55, false) => "underline",
        (0x43, true) => "colors",
        (0x4a, false) => "jump-to-selection",
        (0x52, false) => "ruler",
        (0x54, false) => "font",
        (0x54, true) => "reopen-closed",
        (0x4d, true) => "toggle-format",
        (0x51, false) => "quit",
        (0xbc, false) => "preferences",
        (0x45, false) => "use-selection",
        (0x30, false) => "actual-size",
        (0xbb | 0x6b, false) | (0xbe, true) => "zoom-in",
        (0xbd | 0x6d, false) | (0xbc, true) => "zoom-out",
        (0xbb, true) => "bigger",
        (0xbd, true) => "smaller",
        (0x09, false) | (0x22, false) => "next-tab",
        (0x09, true) | (0x21, false) => "previous-tab",
        (0xba, false) => "check-spelling",
        (0xba, true) => "show-spelling",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vscode_conflicts_have_one_consistent_meaning() {
        let ctrl = Modifiers {
            control: true,
            ..Default::default()
        };
        assert_eq!(shortcut(0x47, ctrl, false), Some("go-to-line"));
        assert_eq!(shortcut(0x4c, ctrl, false), Some("select-line"));
        assert_eq!(shortcut(0xbb, ctrl, false), Some("zoom-in"));
        assert_eq!(shortcut(0x53, ctrl, true), Some("keyboard-help"));
        assert_eq!(
            shortcut(0x72, Modifiers::default(), false),
            Some("find-next")
        );
        assert_eq!(
            shortcut(
                0x26,
                Modifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                },
                false
            ),
            Some("copy-line-up")
        );
    }
    #[test]
    fn altgr_and_plain_typing_are_never_consumed() {
        for key in 0x30..=0x5a {
            assert!(
                shortcut(
                    key,
                    Modifiers {
                        control: true,
                        alt: true,
                        shift: false
                    },
                    false
                )
                .is_none()
            );
            assert!(shortcut(key, Modifiers::default(), false).is_none());
        }
    }
}
