//! Source-code coloring: syntect parsing with bat's grammar and theme collection (two-face).
use std::{path::Path, sync::OnceLock};
use syntect::{
    highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme},
    parsing::{ParseState, ScopeStack, SyntaxSet},
};
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName as T};

/// Code themes offered in View ▸ Code Theme, as (menu label, bundled theme).
pub const THEMES: &[(&str, T)] = &[
    ("Catppuccin Frappé", T::CatppuccinFrappe),
    ("Catppuccin Latte", T::CatppuccinLatte),
    ("Catppuccin Macchiato", T::CatppuccinMacchiato),
    ("Catppuccin Mocha", T::CatppuccinMocha),
    ("Coldark Cold", T::ColdarkCold),
    ("Coldark Dark", T::ColdarkDark),
    ("Dark Neon", T::DarkNeon),
    ("Dracula", T::Dracula),
    ("GitHub", T::Github),
    ("Gruvbox Dark", T::GruvboxDark),
    ("Gruvbox Light", T::GruvboxLight),
    ("Inspired GitHub", T::InspiredGithub),
    ("Monokai Extended", T::MonokaiExtended),
    ("Monokai Extended Bright", T::MonokaiExtendedBright),
    ("Monokai Extended Light", T::MonokaiExtendedLight),
    ("Monokai Extended Origin", T::MonokaiExtendedOrigin),
    ("Nord", T::Nord),
    ("Ocean Dark", T::Base16OceanDark),
    ("Ocean Light", T::Base16OceanLight),
    ("Eighties Dark", T::Base16EightiesDark),
    ("Mocha Dark", T::Base16MochaDark),
    ("One Half Dark", T::OneHalfDark),
    ("One Half Light", T::OneHalfLight),
    ("Solarized Dark", T::SolarizedDark),
    ("Solarized Light", T::SolarizedLight),
    ("Sublime Snazzy", T::SublimeSnazzy),
    ("Two Dark", T::TwoDark),
    ("Zenburn", T::Zenburn),
];
/// Follows the document background: GitHub on light paper, One Half Dark on dark paper.
pub const AUTOMATIC: &str = "Automatic";
/// Larger documents stay uncolored so typing remains responsive.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Like Sublime Text, very long lines (minified files) are not parsed.
const MAX_LINE_BYTES: usize = 10_000;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}
fn themes() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// The grammar name for a file: by file name or extension, then by its first
/// line (shebangs, `<?php`, modelines). `None` means plain text.
pub fn detect(path: &Path, first_line: &str) -> Option<String> {
    let set = syntaxes();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    set.find_syntax_by_extension(name)
        .or_else(|| set.find_syntax_by_extension(extension))
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .filter(|syntax| syntax.name != "Plain Text")
        .map(|syntax| syntax.name.clone())
}

/// Resolves a stored theme preference to a bundled theme.
fn theme_named(name: &str, dark_paper: bool) -> T {
    THEMES
        .iter()
        .find(|(label, _)| *label == name)
        .map(|&(_, theme)| theme)
        .unwrap_or(if dark_paper {
            T::OneHalfDark
        } else {
            T::Github
        })
}
fn rgb(color: syntect::highlighting::Color) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

/// One colored run, in UTF-16 code units as RichEdit counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone)]
struct Line {
    text: String,
    /// Parser and highlighter state before this line.
    parse: ParseState,
    highlight: HighlightState,
}

/// Incremental highlighter for one document: re-colors from the first edited
/// line until the parser state rejoins the unchanged remainder.
pub struct CodeHighlighter {
    syntax: String,
    theme: T,
    lines: Vec<Line>,
}

impl CodeHighlighter {
    pub fn new(syntax: String, theme_name: &str, dark_paper: bool) -> Self {
        Self {
            syntax,
            theme: theme_named(theme_name, dark_paper),
            lines: Vec::new(),
        }
    }
    fn theme(&self) -> &'static Theme {
        themes().get(self.theme)
    }
    /// Document background and default text color for this theme.
    pub fn colors(&self) -> (u32, u32) {
        let settings = &self.theme().settings;
        (
            settings.background.map_or(0xffffff, rgb),
            settings.foreground.map_or(0x000000, rgb),
        )
    }
    /// Spans for every line whose coloring changed since the previous call.
    pub fn update(&mut self, text: &str) -> Vec<Span> {
        let set = syntaxes();
        let Some(syntax) = set.find_syntax_by_name(&self.syntax) else {
            return Vec::new();
        };
        let theme = self.theme();
        let highlighter = Highlighter::new(theme);
        let new: Vec<&str> = text.split_inclusive('\n').collect();
        let old = std::mem::take(&mut self.lines);
        let mut first = old
            .iter()
            .zip(&new)
            .take_while(|(line, text)| line.text == **text)
            .count();
        // Unchanged trailing lines, never overlapping the unchanged prefix.
        let tail = old[first..]
            .iter()
            .rev()
            .zip(new[first..].iter().rev())
            .take_while(|(line, text)| line.text == **text)
            .count();
        // Appending has no cached next line; replay the last line to recover its end state.
        if first == old.len() && first < new.len() {
            first = first.saturating_sub(1);
        }
        let (mut parse, mut highlight) = old.get(first).map_or_else(
            || {
                (
                    ParseState::new(syntax),
                    HighlightState::new(&highlighter, ScopeStack::new()),
                )
            },
            |line| (line.parse.clone(), line.highlight.clone()),
        );
        let mut lines: Vec<Line> = old[..first].to_vec();
        let mut spans = Vec::new();
        let mut offset: usize = new[..first]
            .iter()
            .map(|line| line.encode_utf16().count())
            .sum();
        let mut index = first;
        while index < new.len() {
            // Inside the unchanged tail, stop once the state matches what that line had.
            if index >= new.len() - tail {
                let previous = &old[index + old.len() - new.len()];
                if previous.parse == parse && previous.highlight == highlight {
                    lines.extend_from_slice(&old[index + old.len() - new.len()..]);
                    self.lines = lines;
                    return spans;
                }
            }
            let line = new[index];
            lines.push(Line {
                text: line.to_owned(),
                parse: parse.clone(),
                highlight: highlight.clone(),
            });
            let ops = if line.len() <= MAX_LINE_BYTES {
                parse.parse_line(line, set).ok()
            } else {
                None
            };
            match ops {
                Some(ops) => {
                    for (style, piece) in
                        HighlightIterator::new(&mut highlight, &ops, line, &highlighter)
                    {
                        push_span(&mut spans, offset, piece.encode_utf16().count(), style);
                        offset += piece.encode_utf16().count();
                    }
                }
                None => {
                    // A failed parse may have advanced; keep it paired with the unchanged highlight state.
                    parse = lines[index].parse.clone();
                    let len = line.encode_utf16().count();
                    push_span(&mut spans, offset, len, highlighter.get_default());
                    offset += len;
                }
            }
            index += 1;
        }
        self.lines = lines;
        spans
    }
}

fn push_span(spans: &mut Vec<Span>, start: usize, len: usize, style: syntect::highlighting::Style) {
    if len == 0 {
        return;
    }
    let span = Span {
        start,
        len,
        color: rgb(style.foreground),
        bold: style.font_style.contains(FontStyle::BOLD),
        italic: style.font_style.contains(FontStyle::ITALIC),
    };
    match spans.last_mut() {
        Some(last)
            if last.start + last.len == start
                && (last.color, last.bold, last.italic) == (span.color, span.bold, span.italic) =>
        {
            last.len += len;
        }
        _ => spans.push(span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_and_first_line() {
        assert_eq!(detect(Path::new("main.rs"), "").as_deref(), Some("Rust"));
        assert_eq!(
            detect(Path::new("app.ts"), "").as_deref(),
            Some("TypeScript")
        );
        assert_eq!(
            detect(Path::new("Dockerfile"), "").as_deref(),
            Some("Dockerfile")
        );
        assert_eq!(
            detect(Path::new("run"), "#!/usr/bin/env python3\n").as_deref(),
            Some("Python")
        );
        assert_eq!(detect(Path::new("notes.txt"), "hello"), None);
    }

    #[test]
    fn every_listed_theme_loads() {
        assert!(THEMES.len() >= 12);
        for &(label, _) in THEMES {
            let (background, foreground) =
                CodeHighlighter::new("Rust".into(), label, false).colors();
            assert_ne!(background, foreground, "{label}");
        }
    }

    #[test]
    fn incremental_update_matches_full_highlight() {
        let before = "fn main() {\n    let x = 1;\n}\n";
        let after = "fn main() {\n    let s = \"/* not a comment\";\n    let x = 1;\n}\n";
        let mut incremental = CodeHighlighter::new("Rust".into(), "Dracula", true);
        let first = incremental.update(before);
        assert_eq!(first.iter().map(|s| s.len).sum::<usize>(), before.len());
        let changed = incremental.update(after);
        assert!(!changed.is_empty());
        assert!(
            incremental.update(after).is_empty(),
            "no edit, no recoloring"
        );
        let mut full = CodeHighlighter::new("Rust".into(), "Dracula", true);
        let all = full.update(after);
        // Every changed span equals the corresponding span of a fresh pass.
        for span in &changed {
            assert!(all.iter().any(|s| s.start <= span.start
                && span.start + span.len <= s.start + s.len
                && s.color == span.color));
        }
        // An edit far from the top re-colors only nearby lines.
        let long: String = (0..200).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut lines = CodeHighlighter::new("Rust".into(), AUTOMATIC, false);
        lines.update(&long);
        let edited = long.replacen("let v100 = 100;", "let v100 = 1000;", 1);
        let spans = lines.update(&edited);
        let start = edited.find("let v100").expect("edited line");
        assert!(
            spans
                .iter()
                .all(|s| s.start >= start && s.start < start + 20)
        );
    }
    type UnitStyle = Option<(u32, bool, bool)>;

    fn apply_spans(styles: &mut [UnitStyle], spans: &[Span]) {
        for span in spans {
            let end = span
                .start
                .checked_add(span.len)
                .expect("bounded span length");
            styles
                .get_mut(span.start..end)
                .expect("highlight span stays inside the UTF-16 document")
                .fill(Some((span.color, span.bold, span.italic)));
        }
    }

    fn assert_incremental_matches_fresh(syntax: &str, documents: &[&str]) {
        let mut incremental = CodeHighlighter::new(syntax.into(), "Dracula", true);
        let mut old_units: Vec<u16> = Vec::new();
        let mut actual: Vec<UnitStyle> = Vec::new();
        for (step, text) in documents.iter().enumerate() {
            let new_units: Vec<u16> = text.encode_utf16().collect();
            let prefix = old_units
                .iter()
                .zip(&new_units)
                .take_while(|(old, new)| old == new)
                .count();
            let tail = old_units[prefix..]
                .iter()
                .rev()
                .zip(new_units[prefix..].iter().rev())
                .take_while(|(old, new)| old == new)
                .count();
            // RichEdit shifts retained character formatting with the edited text.
            drop(actual.splice(
                prefix..old_units.len() - tail,
                std::iter::repeat_n(None, new_units.len() - prefix - tail),
            ));
            apply_spans(&mut actual, &incremental.update(text));
            let mut fresh = CodeHighlighter::new(syntax.into(), "Dracula", true);
            let mut expected = vec![None; new_units.len()];
            apply_spans(&mut expected, &fresh.update(text));
            assert_eq!(
                actual, expected,
                "complete UTF-16 styling at step {step}: {text:?}"
            );
            assert!(incremental.update(text).is_empty(), "unchanged step {step}");
            old_units = new_units;
        }
    }

    #[test]
    fn appending_lines_preserves_open_block_comment_state() {
        assert_incremental_matches_fresh(
            "Rust",
            &[
                "/* open 🦀\n",
                "/* open 🦀\ncontinued α\n",
                "/* open 🦀\ncontinued α\n*/\nlet answer = 42;\n",
            ],
        );
    }

    #[test]
    fn appending_lines_preserves_multiline_string_state() {
        assert_incremental_matches_fresh(
            "Python",
            &[
                "message = \"\"\"first 🦀\n",
                "message = \"\"\"first 🦀\nsecond β\n",
                "message = \"\"\"first 🦀\nsecond β\n\"\"\"\nanswer = 42\n",
            ],
        );
    }

    #[test]
    fn deletion_append_and_full_clear_preserve_parser_state() {
        assert_incremental_matches_fresh(
            "Rust",
            &[
                "/* open 🦀\ncontinued α\n*/\nlet answer = 42;\n",
                "/* open 🦀\ncontinued α\n",
                "/* open 🦀\ncontinued α\nmore β\n",
                "/* open 🦀\n",
                "/* open 🦀\n*/\nlet answer = 42;\n",
                "",
                "fn main() {\n    let text = \"🦀 e\u{301}\";\n}\n",
            ],
        );
    }

    #[test]
    fn skipped_long_lines_keep_append_and_delete_updates_consistent() {
        let long_line = format!("{}\n", "x".repeat(MAX_LINE_BYTES + 1));
        let before = format!("/* open 🦀\n{long_line}");
        let appended = format!("{before}continued β\n*/\nlet answer = 42;\n");
        let shortened = appended.replacen(&long_line, "short α\n", 1);
        assert_incremental_matches_fresh("Rust", &[&before, &appended, &shortened]);
    }
    #[test]
    fn appending_a_repeated_final_line_colors_the_new_utf16_units() {
        assert_incremental_matches_fresh(
            "Rust",
            &[
                "/* repeated 🦀\n",
                "/* repeated 🦀\n/* repeated 🦀\n",
                "/* repeated 🦀\n/* repeated 🦀\n*/\n*/\nlet answer = 42;\n",
            ],
        );
    }
}
