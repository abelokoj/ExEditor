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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockStyle {
    #[default]
    Body,
    Heading(u8),
    Blockquote,
    CodeBlock,
}
impl BlockStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Body => "Body",
            Self::Heading(1) => "Heading 1",
            Self::Heading(2) => "Heading 2",
            Self::Heading(3) => "Heading 3",
            Self::Heading(4) => "Heading 4",
            Self::Heading(5) => "Heading 5",
            Self::Heading(_) => "Heading 6",
            Self::Blockquote => "Blockquote",
            Self::CodeBlock => "Code Block",
        }
    }
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Body => "¶",
            Self::Heading(1) => "H1",
            Self::Heading(2) => "H2",
            Self::Heading(3) => "H3",
            Self::Heading(4) => "H4",
            Self::Heading(5) => "H5",
            Self::Heading(_) => "H6",
            Self::Blockquote => "❝",
            Self::CodeBlock => "<>",
        }
    }
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
    FontWeight { bold: bool, italic: bool },
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
    Font(String),
    FontSize(f32),
    Foreground(u32),
    Background(u32),
    Align(Alignment),
    BlockStyle(BlockStyle),
    LineSpacing(f32),
    Indent(i32),
    FirstLineIndent(i32),
    Direction(bool),
    List(ListKind),
    HorizontalRule,
    LineBreak,
    ConvertCase(Case),
    CopyStyle,
    PasteStyle,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FormatState {
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub foreground: u32,
    pub background: u32,
    pub alignment: Alignment,
    pub block_style: BlockStyle,
    pub line_spacing: f32,
}
impl Default for FormatState {
    fn default() -> Self {
        Self {
            font: String::new(),
            size: 0.,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            superscript: false,
            subscript: false,
            foreground: 0,
            background: 0,
            alignment: Alignment::Left,
            block_style: BlockStyle::Body,
            line_spacing: 1.0,
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
    pub show_line_numbers: bool,
    pub line_number_font: String,
    pub line_number_size: f32,
    pub line_number_width: f32,
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
            show_line_numbers: false,
            line_number_font: crate::fonts::default_plain_font().into(),
            line_number_size: 12.,
            line_number_width: 48.,
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
