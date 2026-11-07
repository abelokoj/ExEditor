//! Shared desktop shortcut catalog and chord decoding for GPUI windows.
use std::time::{Duration, Instant};

/// Keyboard modifiers shared by the native and GPUI input paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// One physical key combination; a chord follows the Ctrl+K prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub key: u32,
    pub modifiers: Modifiers,
    pub chord: bool,
    pub action: &'static str,
    /// Canonical display label; aliases use an empty label.
    pub label: &'static str,
}

const NONE: Modifiers = Modifiers {
    control: false,
    shift: false,
    alt: false,
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..NONE
};
const CTRL: Modifiers = Modifiers {
    control: true,
    ..NONE
};
const CTRL_SHIFT: Modifiers = Modifiers {
    control: true,
    shift: true,
    ..NONE
};
const ALT: Modifiers = Modifiers { alt: true, ..NONE };
const ALT_SHIFT: Modifiers = Modifiers {
    alt: true,
    shift: true,
    ..NONE
};

const fn binding(
    key: u32,
    modifiers: Modifiers,
    chord: bool,
    action: &'static str,
    label: &'static str,
) -> Binding {
    Binding {
        key,
        modifiers,
        chord,
        action,
        label,
    }
}

/// Supported shortcuts, including aliases; each action has exactly one canonical label.
pub const BINDINGS: &[Binding] = &[
    binding(0x4b, CTRL, false, "chord", "Ctrl+K"),
    binding(0x4e, CTRL, false, "new-tab", "Ctrl+N"),
    binding(0x4e, CTRL_SHIFT, false, "new", "Ctrl+Shift+N"),
    binding(0x4f, CTRL, false, "open", "Ctrl+O"),
    binding(0x50, CTRL, false, "quick-open", "Ctrl+P"),
    binding(0x52, CTRL, false, "recent-files", "Ctrl+R"),
    binding(0x53, CTRL, false, "save", "Ctrl+S"),
    binding(0x53, CTRL_SHIFT, false, "save-as", "Ctrl+Shift+S"),
    binding(0x57, CTRL, false, "close", "Ctrl+W"),
    binding(0x73, CTRL, false, "close", ""),
    binding(0x57, CTRL_SHIFT, false, "close-window", "Ctrl+Shift+W"),
    binding(0x54, CTRL_SHIFT, false, "reopen-closed", "Ctrl+Shift+T"),
    binding(0x51, CTRL, false, "quit", "Ctrl+Q"),
    binding(0x09, CTRL, false, "next-tab", "Ctrl+Tab"),
    binding(0x22, CTRL, false, "next-tab", ""),
    binding(0x09, CTRL_SHIFT, false, "previous-tab", "Ctrl+Shift+Tab"),
    binding(0x21, CTRL, false, "previous-tab", ""),
    binding(
        0x21,
        CTRL_SHIFT,
        false,
        "move-tab-left",
        "Ctrl+Shift+PageUp",
    ),
    binding(
        0x22,
        CTRL_SHIFT,
        false,
        "move-tab-right",
        "Ctrl+Shift+PageDown",
    ),
    binding(0x5a, CTRL, false, "undo", "Ctrl+Z"),
    binding(0x59, CTRL, false, "redo", "Ctrl+Y"),
    binding(0x5a, CTRL_SHIFT, false, "redo", ""),
    binding(0x43, CTRL, false, "copy", "Ctrl+C"),
    binding(0x58, CTRL, false, "cut", "Ctrl+X"),
    binding(0x56, CTRL, false, "paste", "Ctrl+V"),
    binding(0x56, CTRL_SHIFT, false, "paste-or-preview", "Ctrl+Shift+V"),
    binding(0x41, CTRL, false, "select-all", "Ctrl+A"),
    binding(0x20, CTRL, false, "complete", "Ctrl+Space"),
    binding(0x4c, CTRL, false, "select-line", "Ctrl+L"),
    binding(0x0d, CTRL, false, "insert-line-below", "Ctrl+Enter"),
    binding(
        0x0d,
        CTRL_SHIFT,
        false,
        "insert-line-above",
        "Ctrl+Shift+Enter",
    ),
    binding(0x4b, CTRL_SHIFT, false, "delete-line", "Ctrl+Shift+K"),
    binding(0x26, ALT, false, "move-line-up", "Alt+Up"),
    binding(0x28, ALT, false, "move-line-down", "Alt+Down"),
    binding(0x26, ALT_SHIFT, false, "copy-line-up", "Shift+Alt+Up"),
    binding(0x28, ALT_SHIFT, false, "copy-line-down", "Shift+Alt+Down"),
    binding(0xdd, CTRL, false, "indent-line", "Ctrl+]"),
    binding(0xdb, CTRL, false, "outdent-line", "Ctrl+["),
    binding(0xbf, CTRL, false, "toggle-comment", "Ctrl+/"),
    binding(0x41, ALT_SHIFT, false, "block-comment", "Shift+Alt+A"),
    binding(0x47, CTRL, false, "go-to-line", "Ctrl+G"),
    binding(0xdc, CTRL_SHIFT, false, "matching-bracket", "Ctrl+Shift+\\"),
    binding(0x4a, CTRL, false, "jump-to-selection", "Ctrl+J"),
    binding(0x46, CTRL, false, "find", "Ctrl+F"),
    binding(0x48, CTRL, false, "replace", "Ctrl+H"),
    binding(0x72, NONE, false, "find-next", "F3"),
    binding(0x72, SHIFT, false, "find-previous", "Shift+F3"),
    binding(0x45, CTRL, false, "use-selection", "Ctrl+E"),
    binding(0x26, CTRL, false, "scroll-line-up", "Ctrl+Up"),
    binding(0x28, CTRL, false, "scroll-line-down", "Ctrl+Down"),
    binding(0x21, ALT, false, "scroll-page-up", "Alt+PageUp"),
    binding(0x22, ALT, false, "scroll-page-down", "Alt+PageDown"),
    binding(0x5a, ALT, false, "word-wrap", "Alt+Z"),
    binding(0x7a, NONE, false, "fullscreen", "F11"),
    binding(0xbb, CTRL, false, "zoom-in", "Ctrl+="),
    binding(0xbb, CTRL_SHIFT, false, "zoom-in", ""),
    binding(0x6b, CTRL, false, "zoom-in", ""),
    binding(0x6b, CTRL_SHIFT, false, "zoom-in", ""),
    binding(0xbd, CTRL, false, "zoom-out", "Ctrl+-"),
    binding(0x6d, CTRL, false, "zoom-out", ""),
    binding(0x30, CTRL, false, "actual-size", "Ctrl+0"),
    binding(0x42, CTRL, false, "bold", "Ctrl+B"),
    binding(0x49, CTRL, false, "italic", "Ctrl+I"),
    binding(0x55, CTRL, false, "underline", "Ctrl+U"),
    binding(0x43, CTRL_SHIFT, false, "colors", "Ctrl+Shift+C"),
    binding(0x4d, CTRL_SHIFT, false, "toggle-format", "Ctrl+Shift+M"),
    binding(0x52, CTRL_SHIFT, false, "ruler", "Ctrl+Shift+R"),
    binding(0x54, CTRL, false, "font", "Ctrl+T"),
    binding(0xbe, CTRL_SHIFT, false, "bigger", "Ctrl+Shift+."),
    binding(0xbc, CTRL_SHIFT, false, "smaller", "Ctrl+Shift+,"),
    binding(0xbd, CTRL_SHIFT, false, "smaller", ""),
    binding(0x6d, CTRL_SHIFT, false, "smaller", ""),
    binding(0xba, CTRL, false, "check-spelling", "Ctrl+;"),
    binding(0xba, CTRL_SHIFT, false, "show-spelling", "Ctrl+Shift+;"),
    binding(0x50, CTRL_SHIFT, false, "command-palette", "Ctrl+Shift+P"),
    binding(0x70, NONE, false, "command-palette", ""),
    binding(0xbc, CTRL, false, "preferences", "Ctrl+,"),
    binding(0x46, ALT, false, "file-menu", "Alt+F"),
    binding(0x79, NONE, false, "file-menu", ""),
    binding(0x45, ALT, false, "edit-menu", "Alt+E"),
    binding(0x4f, ALT, false, "format-menu", "Alt+O"),
    binding(0x56, ALT, false, "view-menu", "Alt+V"),
    binding(0x57, ALT, false, "window-menu", "Alt+W"),
    binding(0x48, ALT, false, "help-menu", "Alt+H"),
    binding(0x53, NONE, true, "save-all", "Ctrl+K S"),
    binding(0x53, CTRL, true, "keyboard-help", "Ctrl+K Ctrl+S"),
    binding(0x43, CTRL, true, "add-comment", "Ctrl+K Ctrl+C"),
    binding(0x55, CTRL, true, "remove-comment", "Ctrl+K Ctrl+U"),
    binding(0x58, CTRL, true, "trim-whitespace", "Ctrl+K Ctrl+X"),
    binding(0x57, CTRL, true, "close-all", "Ctrl+K Ctrl+W"),
    binding(0x56, NONE, true, "markdown-preview", "Ctrl+K V"),
    binding(0x50, NONE, true, "copy-path", "Ctrl+K P"),
    binding(0x52, NONE, true, "reveal-file", "Ctrl+K R"),
    binding(0x50, CTRL, true, "print", "Ctrl+K Ctrl+P"),
    binding(0x50, CTRL_SHIFT, true, "page-setup", "Ctrl+K Ctrl+Shift+P"),
];

/// Returns the canonical display label, or an empty string for an unbound action.
pub fn label(action: &str) -> &'static str {
    BINDINGS
        .iter()
        .find(|binding| binding.action == action && !binding.label.is_empty())
        .map_or("", |binding| binding.label)
}

/// Resolves one key against the catalog without changing chord state.
pub fn shortcut(key: u32, modifiers: Modifiers, chord: bool) -> Option<&'static str> {
    if modifiers.control && modifiers.alt {
        return None;
    }
    BINDINGS
        .iter()
        .find(|binding| {
            binding.key == key && binding.modifiers == modifiers && binding.chord == chord
        })
        .map(|binding| binding.action)
}

/// The input decision shared by both keyboard paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Leave the key available to ordinary text input or native navigation.
    Ignored,
    /// Consume Ctrl+K and wait for its second key.
    Prefix,
    /// Consume Escape or an invalid second key, ending the pending chord.
    Cancelled,
    /// Consume the key and dispatch its command.
    Action(&'static str),
}

/// Pending Ctrl+K state, kept independently for each document window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    chord_started: Option<Instant>,
}

impl State {
    /// The same decoder serves the editor window and menus on every desktop platform.
    pub fn resolve_keystroke(&mut self, keystroke: &gpui_kit::Keystroke, now: Instant) -> Outcome {
        let m = keystroke.modifiers;
        let modifiers = modifiers_for_key(
            &keystroke.key,
            Modifiers {
                control: m.control || m.platform,
                shift: m.shift,
                alt: m.alt,
            },
        );
        self.resolve(virtual_key(&keystroke.key).unwrap_or(0), modifiers, now)
    }
    /// Cancels any pending chord, for example when focus changes.
    pub fn reset(&mut self) {
        self.chord_started = None;
    }

    /// Resolves a key, expiring a pending chord after three seconds.
    pub fn resolve(&mut self, key: u32, modifiers: Modifiers, now: Instant) -> Outcome {
        if modifiers.control && modifiers.alt {
            self.reset();
            return Outcome::Ignored;
        }
        if matches!(key, 0x10..=0x12 | 0x5b..=0x5c | 0xa0..=0xa5) {
            return Outcome::Ignored;
        }
        let pending = self
            .chord_started
            .take()
            .is_some_and(|started| now.saturating_duration_since(started) < Duration::from_secs(3));
        if pending {
            return shortcut(key, modifiers, true).map_or(Outcome::Cancelled, Outcome::Action);
        }
        match shortcut(key, modifiers, false) {
            Some("chord") => {
                self.chord_started = Some(now);
                Outcome::Prefix
            }
            Some(action) => Outcome::Action(action),
            None => Outcome::Ignored,
        }
    }
}

/// Windows virtual-key code for a GPUI key name or its shifted character.
pub fn virtual_key(key: &str) -> Option<u32> {
    Some(match key {
        "tab" => 0x09,
        "enter" | "return" => 0x0d,
        "escape" | "esc" => 0x1b,
        "space" | " " => 0x20,
        "backspace" => 0x08,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "home" => 0x24,
        "end" => 0x23,
        "insert" => 0x2d,
        "delete" => 0x2e,
        "shift" => 0x10,
        "control" | "ctrl" => 0x11,
        "alt" => 0x12,
        "lshift" => 0xa0,
        "rshift" => 0xa1,
        "lcontrol" => 0xa2,
        "rcontrol" => 0xa3,
        "lalt" => 0xa4,
        "ralt" => 0xa5,
        "super" | "meta" | "lwin" => 0x5b,
        "rwin" => 0x5c,
        "add" | "numpadadd" | "numpad+" => 0x6b,
        "subtract" | "numpadsubtract" | "numpad-" => 0x6d,
        "/" | "?" => 0xbf,
        "," | "<" => 0xbc,
        "." | ">" => 0xbe,
        "=" | "+" => 0xbb,
        "-" | "_" => 0xbd,
        ";" | ":" => 0xba,
        "[" | "{" => 0xdb,
        "\\" | "|" => 0xdc,
        "]" | "}" => 0xdd,
        "'" | "\"" => 0xde,
        "`" | "~" => 0xc0,
        "!" => 0x31,
        "@" => 0x32,
        "#" => 0x33,
        "$" => 0x34,
        "%" => 0x35,
        "^" => 0x36,
        "&" => 0x37,
        "*" => 0x38,
        "(" => 0x39,
        ")" => 0x30,
        _ => match key.as_bytes() {
            [c @ (b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')] => u32::from(c.to_ascii_uppercase()),
            _ => {
                let number = key
                    .strip_prefix('f')
                    .or_else(|| key.strip_prefix('F'))?
                    .parse::<u32>()
                    .ok()?;
                if !(1..=24).contains(&number) {
                    return None;
                }
                0x6f + number
            }
        },
    })
}

/// Restores Shift when GPUI represents shifted punctuation as a character.
pub fn modifiers_for_key(key: &str, mut modifiers: Modifiers) -> Modifiers {
    // GPUI clears Shift after converting Windows OEM keys to their shifted character.
    if matches!(
        key,
        "~" | "!"
            | "@"
            | "#"
            | "$"
            | "%"
            | "^"
            | "&"
            | "*"
            | "("
            | ")"
            | "_"
            | "+"
            | "{"
            | "}"
            | "|"
            | ":"
            | "\""
            | "<"
            | ">"
            | "?"
    ) {
        modifiers.shift = true;
    }
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_ambiguous_combinations_or_labels() {
        for (index, binding) in BINDINGS.iter().enumerate() {
            assert!(
                !binding.modifiers.control || !binding.modifiers.alt,
                "{binding:?}"
            );
            assert_eq!(
                shortcut(binding.key, binding.modifiers, binding.chord),
                Some(binding.action),
                "{binding:?}"
            );
            for other in &BINDINGS[index + 1..] {
                assert!(
                    (binding.key, binding.modifiers, binding.chord)
                        != (other.key, other.modifiers, other.chord),
                    "duplicate combinations: {binding:?}, {other:?}"
                );
            }
            assert_eq!(
                BINDINGS
                    .iter()
                    .filter(|other| { other.action == binding.action && !other.label.is_empty() })
                    .count(),
                1,
                "action needs exactly one primary label: {binding:?}"
            );
            assert!(!label(binding.action).is_empty(), "{binding:?}");
        }
        assert_eq!(label("unbound-command"), "");
    }

    #[test]
    fn every_catalog_entry_decodes_in_a_fresh_state() {
        let now = Instant::now();
        for binding in BINDINGS {
            let mut state = State::default();
            if binding.chord {
                assert_eq!(state.resolve(0x4b, CTRL, now), Outcome::Prefix);
            }
            let expected = if binding.action == "chord" {
                Outcome::Prefix
            } else {
                Outcome::Action(binding.action)
            };
            assert_eq!(
                state.resolve(binding.key, binding.modifiers, now),
                expected,
                "{binding:?}"
            );
        }
    }

    #[test]
    fn canonical_labels_match_their_key_combinations() {
        for binding in BINDINGS.iter().filter(|binding| !binding.label.is_empty()) {
            let combination = if let Some((prefix, key)) = binding.label.split_once(' ') {
                assert_eq!(prefix, "Ctrl+K", "{binding:?}");
                assert!(binding.chord, "{binding:?}");
                key
            } else {
                assert!(!binding.chord, "{binding:?}");
                binding.label
            };
            let mut parts: Vec<_> = combination.split('+').collect();
            let key_name = parts.pop().expect("canonical labels contain a key");
            let modifiers = Modifiers {
                control: parts.contains(&"Ctrl"),
                shift: parts.contains(&"Shift"),
                alt: parts.contains(&"Alt"),
            };
            assert!(
                parts
                    .iter()
                    .all(|part| matches!(*part, "Ctrl" | "Shift" | "Alt"))
            );
            assert_eq!(
                virtual_key(&key_name.to_ascii_lowercase()),
                Some(binding.key),
                "{binding:?}"
            );
            assert_eq!(modifiers, binding.modifiers, "{binding:?}");
        }
    }
    #[test]
    fn help_includes_every_primary_binding() {
        let help = include_str!("../docs/KEYBOARD_SHORTCUTS.txt");
        for binding in BINDINGS.iter().filter(|binding| !binding.label.is_empty()) {
            assert!(help.contains(binding.label), "missing help for {binding:?}");
        }
    }

    #[test]
    fn vscode_conflicts_keep_the_requested_editor_meaning() {
        for (key, modifiers, chord, action) in [
            (0x50, CTRL, false, "quick-open"),
            (0x4f, CTRL, false, "open"),
            (0x52, CTRL, false, "recent-files"),
            (0x52, CTRL_SHIFT, false, "ruler"),
            (0x20, CTRL, false, "complete"),
            (0x42, CTRL, false, "bold"),
            (0x49, CTRL, false, "italic"),
            (0x55, CTRL, false, "underline"),
            (0xbb, CTRL_SHIFT, false, "zoom-in"),
            (0x6b, CTRL_SHIFT, false, "zoom-in"),
            (0xbe, CTRL_SHIFT, false, "bigger"),
            (0xbc, CTRL_SHIFT, false, "smaller"),
            (0xbd, CTRL_SHIFT, false, "smaller"),
            (0x53, NONE, true, "save-all"),
            (0x53, CTRL, true, "keyboard-help"),
            (0x50, CTRL, true, "print"),
            (0x50, CTRL_SHIFT, true, "page-setup"),
        ] {
            assert_eq!(shortcut(key, modifiers, chord), Some(action));
        }
    }

    #[test]
    fn every_chord_survives_modifier_transitions_without_extending_its_timeout() {
        let now = Instant::now();
        for binding in BINDINGS.iter().filter(|binding| binding.chord) {
            for modifier in [
                0x10, 0x11, 0x12, 0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
            ] {
                let mut state = State::default();
                assert_eq!(state.resolve(0x4b, CTRL, now), Outcome::Prefix);
                assert_eq!(
                    state.resolve(modifier, NONE, now + Duration::from_secs(2)),
                    Outcome::Ignored
                );
                assert_eq!(
                    state.resolve(
                        binding.key,
                        binding.modifiers,
                        now + Duration::from_millis(2999)
                    ),
                    Outcome::Action(binding.action),
                    "modifier {modifier:x}, {binding:?}"
                );
                let mut expired = State::default();
                expired.resolve(0x4b, CTRL, now);
                expired.resolve(modifier, NONE, now + Duration::from_secs(2));
                assert_eq!(
                    expired.resolve(binding.key, binding.modifiers, now + Duration::from_secs(3)),
                    shortcut(binding.key, binding.modifiers, false)
                        .map_or(Outcome::Ignored, Outcome::Action),
                    "modifier {modifier:x} extended the timeout for {binding:?}"
                );
            }
        }
    }

    #[test]
    fn altgr_clears_chords_and_always_passes_through() {
        let now = Instant::now();
        for shift in [false, true] {
            let altgr = Modifiers {
                control: true,
                alt: true,
                shift,
            };
            for key in 0..=0xff {
                for pending in [false, true] {
                    let mut state = State::default();
                    if pending {
                        state.resolve(0x4b, CTRL, now);
                    }
                    assert_eq!(shortcut(key, altgr, pending), None, "key {key:x}");
                    assert_eq!(
                        state.resolve(key, altgr, now),
                        Outcome::Ignored,
                        "key {key:x}"
                    );
                    assert_eq!(
                        state.resolve(0x53, NONE, now),
                        Outcome::Ignored,
                        "key {key:x}"
                    );
                }
            }
        }
    }

    #[test]
    fn ordinary_typing_and_native_navigation_pass_through() {
        let now = Instant::now();
        for modifiers in [NONE, SHIFT] {
            for key in 0x20..=0x6f {
                assert_eq!(
                    State::default().resolve(key, modifiers, now),
                    Outcome::Ignored,
                    "typing key {key:x}, {modifiers:?}"
                );
            }
        }
        for modifiers in [NONE, SHIFT, CTRL, CTRL_SHIFT] {
            for key in [0x08, 0x23, 0x24, 0x25, 0x27, 0x2e] {
                assert_eq!(
                    State::default().resolve(key, modifiers, now),
                    Outcome::Ignored,
                    "native navigation key {key:x}, {modifiers:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_chord_keys_are_consumed_once() {
        let now = Instant::now();
        for key in 0..=0xff {
            if matches!(key, 0x10..=0x12 | 0x5b..=0x5c | 0xa0..=0xa5) {
                continue;
            }
            for modifiers in [NONE, SHIFT, CTRL, CTRL_SHIFT, ALT, ALT_SHIFT] {
                if shortcut(key, modifiers, true).is_some() {
                    continue;
                }
                let mut state = State::default();
                state.resolve(0x4b, CTRL, now);
                assert_eq!(
                    state.resolve(key, modifiers, now),
                    Outcome::Cancelled,
                    "key {key:x}"
                );
                assert_eq!(
                    state.resolve(0x53, NONE, now),
                    Outcome::Ignored,
                    "key {key:x}"
                );
            }
        }
    }

    #[test]
    fn escape_reset_and_timeout_end_the_prefix() {
        let now = Instant::now();
        let mut state = State::default();
        assert_eq!(state.resolve(0x1b, NONE, now), Outcome::Ignored);
        state.resolve(0x4b, CTRL, now);
        assert_eq!(state.resolve(0x1b, NONE, now), Outcome::Cancelled);
        assert_eq!(state.resolve(0x53, CTRL, now), Outcome::Action("save"));
        state.resolve(0x4b, CTRL, now);
        state.reset();
        assert_eq!(state.resolve(0x53, NONE, now), Outcome::Ignored);
        state.resolve(0x4b, CTRL, now);
        assert_eq!(
            state.resolve(0x53, CTRL, now + Duration::from_secs(3)),
            Outcome::Action("save")
        );
        state.resolve(0x4b, CTRL, now);
        assert_eq!(
            state.resolve(0x53, NONE, now + Duration::from_secs(4)),
            Outcome::Ignored
        );
    }

    #[test]
    fn gpui_key_aliases_cover_the_catalog() {
        for key in b'a'..=b'z' {
            assert_eq!(
                virtual_key(&char::from(key).to_string()),
                Some(u32::from(key.to_ascii_uppercase()))
            );
            assert_eq!(
                virtual_key(&char::from(key.to_ascii_uppercase()).to_string()),
                Some(u32::from(key.to_ascii_uppercase()))
            );
        }
        for key in b'0'..=b'9' {
            assert_eq!(
                virtual_key(&char::from(key).to_string()),
                Some(u32::from(key))
            );
        }
        for number in 1..=24 {
            assert_eq!(virtual_key(&format!("f{number}")), Some(0x6f + number));
            assert_eq!(virtual_key(&format!("F{number}")), Some(0x6f + number));
        }
        for (names, key) in [
            (&["space", " "][..], 0x20),
            (&["escape", "esc"][..], 0x1b),
            (&["enter", "return"][..], 0x0d),
            (&["/", "?"][..], 0xbf),
            (&[",", "<"][..], 0xbc),
            (&[".", ">"][..], 0xbe),
            (&["=", "+"][..], 0xbb),
            (&["-", "_"][..], 0xbd),
            (&[";", ":"][..], 0xba),
            (&["[", "{"][..], 0xdb),
            (&["\\", "|"][..], 0xdc),
            (&["]", "}"][..], 0xdd),
            (&["add", "numpadadd", "numpad+"][..], 0x6b),
            (&["subtract", "numpadsubtract", "numpad-"][..], 0x6d),
        ] {
            for name in names {
                assert_eq!(virtual_key(name), Some(key), "{name}");
            }
        }
        for name in ["f0", "f25", "unknown", "", "é"] {
            assert_eq!(virtual_key(name), None, "{name}");
        }
    }

    #[test]
    fn shifted_gpui_punctuation_resolves_like_native_keys() {
        for (name, native_key, action) in [
            (">", 0xbe, "bigger"),
            ("<", 0xbc, "smaller"),
            ("|", 0xdc, "matching-bracket"),
            ("+", 0xbb, "zoom-in"),
            ("_", 0xbd, "smaller"),
            (":", 0xba, "show-spelling"),
        ] {
            let modifiers = modifiers_for_key(name, CTRL);
            assert_eq!(modifiers, CTRL_SHIFT, "{name}");
            assert_eq!(virtual_key(name), Some(native_key), "{name}");
            assert_eq!(
                shortcut(native_key, modifiers, false),
                Some(action),
                "{name}"
            );
        }
        for name in ["a", "A", ".", ",", "=", "-", "\\", "space"] {
            assert_eq!(modifiers_for_key(name, CTRL), CTRL, "{name}");
            assert_eq!(modifiers_for_key(name, CTRL_SHIFT), CTRL_SHIFT, "{name}");
        }
    }
}
