use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    None,
    Bullet,
    Number,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Title,
}
#[derive(Clone, Debug)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PastePlain,
    SelectAll,
    Delete,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
    Font(String),
    FontSize(f32),
    Foreground(u32),
    Background(u32),
    Align(Alignment),
    LineSpacing(f32),
    Indent(i32),
    FirstLineIndent(i32),
    TabStop(i32),
    Direction(bool),
    List(ListKind),
    ConvertCase(Case),
    CopyStyle,
    PasteStyle,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatState {
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub foreground: u32,
    pub background: u32,
    pub alignment: Alignment,
}
#[derive(Clone, Debug)]
pub struct PageSettings {
    pub width_twips: i32,
    pub height_twips: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    pub margin_top: i32,
    pub margin_bottom: i32,
}
impl Default for PageSettings {
    fn default() -> Self {
        Self {
            width_twips: 12_240,
            height_twips: 15_840,
            margin_left: 1_080,
            margin_right: 1_080,
            margin_top: 1_080,
            margin_bottom: 1_080,
        }
    }
}
/// Screen-only state; none of these values changes the saved document or its undo history.
#[derive(Clone, Debug)]
pub struct EditorDisplayState {
    pub revision: u64,
    pub zoom: u16,
    pub wrap: bool,
    pub wrap_to_page: bool,
    pub background: u32,
    pub text_override: Option<u32>,
    pub syntax: Vec<crate::syntax::Span>,
    pub proofing: Vec<Range<usize>>,
    pub read_only: bool,
    pub visible: bool,
    pub auto_links: bool,
    pub plain: bool,
    pub autocomplete_enabled: bool,
    pub completion_language: String,
}
impl Default for EditorDisplayState {
    fn default() -> Self {
        Self {
            revision: 0,
            zoom: 100,
            wrap: true,
            wrap_to_page: false,
            background: 0xffffff,
            text_override: None,
            syntax: Vec::new(),
            proofing: Vec::new(),
            read_only: false,
            visible: true,
            auto_links: true,
            plain: false,
            autocomplete_enabled: true,
            completion_language: String::new(),
        }
    }
}
