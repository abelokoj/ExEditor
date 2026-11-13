//! Native NSH editing sessions. Public ranges count UTF-16 code units.
use anyhow::{Context as _, Result, bail, ensure};
use nsh_base::rich_editor::{
    commands::{BoxCommand, tiptap},
    extensions::{
        Extension, ExtensionManager, GlobalAttributes, kit,
        link::{self, LinkOptions},
    },
    formats::{DocFormat, DomParser, ParseOptions, PlainTextFormat, doc_to_html, slice_from_text},
    model::{AttrParse, AttrRender, AttrSpec, Attrs, Fragment, Node, Slice},
    state::{EditorState, EditorStateConfig, Plugin, TextSelection},
    view::{EditorSession, NodeViews, SessionEventKind},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ops::Range,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use wordcraft_doc::Document;

pub mod clipboard;
mod core_positions;
mod overlays;
pub mod printing;
pub mod surface;
mod types;
pub use types::{
    Alignment, Case, EditCommand, EditorDisplayState, FormatState, ListKind, PageSettings,
};
const MAX_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
thread_local! { static COPIED_STYLE: RefCell<Option<FormatState>> = const { RefCell::new(None) }; }
#[derive(Default)]
struct Changes {
    modified: AtomicBool,
    buffered: AtomicBool,
    text: AtomicBool,
    loading: AtomicBool,
    revision: AtomicU64,
}

#[derive(Clone)]
struct CompletionPopup {
    result: crate::completion::Completion,
    selected: usize,
    key: (u64, usize, usize),
}

/// A stable native session survives tab switches and retains its selection and undo history.
pub struct Editor {
    session: EditorSession,
    manager: RefCell<ExtensionManager>,
    changes: Arc<Changes>,
    overlays: Arc<Mutex<overlays::Overlays>>,
    display: RefCell<EditorDisplayState>,
    display_revision: Cell<u64>,
    page: RefCell<PageSettings>,
    plain_style: RefCell<FormatState>,
    pasted: Cell<bool>,
    focus_requested: Cell<bool>,
    scroll_requested: Cell<bool>,
    scroll_delta: Cell<(i32, bool)>,
    view_scroll: Cell<(f32, f32)>,
    viewport_width: Cell<f32>,
    shortcuts: RefCell<VecDeque<String>>,
    completion: RefCell<Option<CompletionPopup>>,
    completion_seen: Cell<Option<(u64, usize, usize)>>,
}
fn manager(plain: bool, auto_links: bool) -> Result<ExtensionManager> {
    let extensions = if plain {
        vec![
            kit::document(),
            kit::paragraph(),
            kit::text(),
            kit::hard_break(),
            kit::history(),
        ]
    } else {
        vec![kit::starter_kit().configure(json!({"link":{"autolink":auto_links,"linkOnPaste":auto_links}, "typography":false,"image":false})), kit::table_kit()]
    };
    let mut extensions = extensions;
    if !plain {
        extensions.push(kit::image().add_input_rules(|_| Vec::new()));
        extensions.push(
            Extension::new("exeditorParagraph").add_global_attributes(|_| {
                vec![GlobalAttributes::new(
                    vec!["paragraph".into(), "heading".into()],
                    [
                        ("lineHeight", "line-height"),
                        ("indent", "margin-left"),
                        ("direction", "direction"),
                    ]
                    .into_iter()
                    .map(|(name, css)| {
                        (
                            name.into(),
                            AttrSpec {
                                default: Some(Value::Null),
                                parse_html: Some(AttrParse::Style(css.into())),
                                render_html: Some(AttrRender::Style(css.into())),
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
                )]
            }),
        );
    }
    ExtensionManager::with_core(extensions).map_err(anyhow::Error::new)
}
fn plugins(
    manager: &ExtensionManager,
    overlay: Arc<Mutex<overlays::Overlays>>,
    plain: bool,
    auto_links: bool,
) -> Vec<Plugin> {
    let mut plugins = manager.plugins();
    if !plain {
        let mut options = LinkOptions::default();
        options.autolink = auto_links;
        options.link_on_paste = auto_links;
        plugins.extend(link::link(options));
    }
    plugins.insert(0, Arc::new(overlays::OverlayPlugin(overlay)));
    plugins
}
impl Editor {
    pub fn new() -> Result<Self> {
        let manager = manager(false, true)?;
        let overlays = Arc::new(Mutex::new(overlays::Overlays::default()));
        let state = EditorState::create(
            EditorStateConfig::new(manager.schema().clone()).plugins(plugins(
                &manager,
                overlays.clone(),
                false,
                true,
            )),
        )?;
        let session = EditorSession::new(state, NodeViews::new());
        let changes = Arc::new(Changes::default());
        let updated = changes.clone();
        session.on(
            SessionEventKind::Update,
            Arc::new(move |_| {
                if !updated.loading.load(Ordering::Relaxed) {
                    updated.modified.store(true, Ordering::Relaxed);
                    updated.buffered.store(true, Ordering::Relaxed);
                    updated.text.store(true, Ordering::Relaxed);
                    updated.revision.fetch_add(1, Ordering::Relaxed);
                }
            }),
        );
        Ok(Self {
            session,
            manager: RefCell::new(manager),
            changes,
            overlays,
            display: RefCell::new(EditorDisplayState::default()),
            display_revision: Cell::new(1),
            page: RefCell::new(PageSettings::default()),
            plain_style: RefCell::new(FormatState {
                font: crate::fonts::default_plain_font().into(),
                size: 13.,
                foreground: 0,
                background: 0xffffff,
                ..Default::default()
            }),
            pasted: Cell::new(false),
            focus_requested: Cell::new(false),
            scroll_requested: Cell::new(false),
            scroll_delta: Cell::new((0, false)),
            view_scroll: Cell::new((0., 0.)),
            viewport_width: Cell::new(600.),
            shortcuts: RefCell::new(VecDeque::new()),
            completion: RefCell::new(None),
            completion_seen: Cell::new(None),
        })
    }
    pub fn report_error(&self, error: String) {
        self.overlays
            .lock()
            .expect("editor overlay lock poisoned")
            .error = Some(error);
    }
    pub fn take_error(&self) -> Option<String> {
        self.overlays
            .lock()
            .expect("editor overlay lock poisoned")
            .error
            .take()
    }
    pub fn native_session(&self) -> EditorSession {
        self.session.clone()
    }
    /// JSON of the native document, never a WordCraft recovery payload.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(
            &json!({"doc":self.session.state().doc().to_json(),"page":self.page_settings()}),
        )
        .ok()
    }
    pub fn set_snapshot(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_DOCUMENT_BYTES,
            "the recovery document is too large"
        );
        let value: Value =
            serde_json::from_slice(bytes).context("reading the native recovery document")?;
        let doc = self
            .manager
            .borrow()
            .schema()
            .node_from_json(value.get("doc").unwrap_or(&value))?;
        let page = value
            .get("page")
            .map(|value| serde_json::from_value::<PageSettings>(value.clone()))
            .transpose()?;
        if let Some(page) = page {
            printing::validate_page_settings(&page)?;
            *self.page.borrow_mut() = page;
        }
        doc.check()?;
        self.load(doc)
    }
    /// Converts only at an import/export or printing boundary.
    pub fn document_snapshot(&self) -> Result<Document> {
        let mut doc = wordcraft_formats::html::import_page(&self.html()?, &|_| None);
        doc.ensure_nonempty();
        printing::apply_page_settings(&mut doc, &self.page.borrow())?;
        Ok(doc)
    }
    pub fn html(&self) -> Result<String> {
        Ok(doc_to_html(self.session.state().doc()))
    }
    pub fn document_revision(&self) -> u64 {
        self.changes.revision.load(Ordering::Relaxed)
    }
    pub fn revision(&self) -> u64 {
        self.session
            .state()
            .revision()
            .wrapping_mul(1_000_003)
            .wrapping_add(self.display_revision.get())
    }
    fn invalidate_display(&self) {
        self.display_revision
            .set(self.display_revision.get().wrapping_add(1));
    }
    fn map(&self) -> core_positions::TextMap {
        core_positions::TextMap::new(self.session.state().doc())
    }
    pub fn text(&self) -> Result<String> {
        Ok(self.map().text)
    }
    pub fn plain_text_snapshot(&self) -> String {
        self.map().text
    }
    pub fn text_length(&self) -> usize {
        self.map().len()
    }
    pub fn position_at_utf16(&self, offset: usize) -> Result<usize> {
        self.map().position(offset)
    }
    pub fn utf16_at_position(&self, position: &usize) -> Option<usize> {
        Some(self.map().offset(*position))
    }
    pub fn selection(&self) -> Range<usize> {
        let state = self.session.state();
        let map = self.map();
        map.offset(state.selection().from())..map.offset(state.selection().to())
    }
    pub fn select_range(&self, range: Range<usize>) -> Result<()> {
        ensure!(
            range.start <= range.end,
            "the requested selection is reversed"
        );
        let map = self.map();
        self.set_document_selection(map.position(range.start)?, map.position(range.end)?)
    }
    pub fn set_document_selection(&self, anchor: usize, focus: usize) -> Result<()> {
        let state = self.session.state();
        ensure!(
            anchor <= state.doc().content().size() && focus <= state.doc().content().size(),
            "the selection is outside the document"
        );
        let mut tr = state.tr();
        tr.set_selection(TextSelection::create(state.doc(), anchor, Some(focus)));
        self.session.dispatch(tr)?;
        Ok(())
    }
    pub fn selected_text(&self) -> Result<String> {
        let map = self.map();
        Ok(map.slice(self.selection())?.into())
    }
    fn load(&self, doc: Node) -> Result<()> {
        validate_native_resources(&doc)?;
        ensure!(
            doc.to_json().to_string().len() <= MAX_DOCUMENT_BYTES,
            "the document is too large"
        );
        doc.check()?;
        let manager = self.manager.borrow();
        let state = EditorState::create(
            EditorStateConfig::new(manager.schema().clone())
                .doc(doc)
                .plugins(plugins(
                    &manager,
                    self.overlays.clone(),
                    self.is_plain(),
                    self.display.borrow().auto_links,
                )),
        )?;
        self.changes.loading.store(true, Ordering::Relaxed);
        self.session.update_state(state);
        self.changes.loading.store(false, Ordering::Relaxed);
        self.changes.revision.fetch_add(1, Ordering::Relaxed);
        self.mark_saved();
        self.changes.text.store(true, Ordering::Relaxed);
        self.display.borrow_mut().syntax.clear();
        self.display.borrow_mut().proofing.clear();
        self.view_scroll.set((0., 0.));
        self.invalidate_display();
        Ok(())
    }
    pub fn set_document(&self, document: Document) -> Result<()> {
        let section = &document.last_section;
        *self.page.borrow_mut() = PageSettings {
            width_twips: (section.page_w * 20.).round() as i32,
            height_twips: (section.page_h * 20.).round() as i32,
            margin_left: (section.margin_left * 20.).round() as i32,
            margin_right: (section.margin_right * 20.).round() as i32,
            margin_top: (section.margin_top * 20.).round() as i32,
            margin_bottom: (section.margin_bottom * 20.).round() as i32,
        };
        let html = wordcraft_formats::html::export(&document);
        let doc = DomParser::from_schema(self.manager.borrow().schema())
            .parse(&html, &ParseOptions::default());
        self.load(doc)
    }
    pub fn set_text(&self, text: &str) -> Result<()> {
        validate_text(text)?;
        let doc = PlainTextFormat::default()
            .parse(self.manager.borrow().schema(), &normalize_newlines(text))?;
        self.load(doc)
    }
    pub fn rtf(&self) -> Result<Vec<u8>> {
        Ok(wordcraft_formats::rtf::export(&self.document_snapshot()?).into_bytes())
    }
    pub fn set_rtf(&self, bytes: &[u8]) -> Result<()> {
        validate_rtf(bytes)?;
        self.set_document(
            wordcraft_formats::rtf::import(bytes.trim_ascii_start()).map_err(anyhow::Error::msg)?,
        )
    }
    pub fn is_modified(&self) -> bool {
        self.changes.modified.load(Ordering::Relaxed)
    }
    pub fn has_buffered_changes(&self) -> bool {
        self.changes.buffered.load(Ordering::Relaxed)
    }
    pub fn mark_saved(&self) {
        self.changes.modified.store(false, Ordering::Relaxed);
        self.changes.buffered.store(false, Ordering::Relaxed);
    }
    pub fn acknowledge_buffered_changes(&self) {
        self.changes.buffered.store(false, Ordering::Relaxed);
    }
    pub fn mark_modified(&self) {
        self.changes.modified.store(true, Ordering::Relaxed);
        self.changes.buffered.store(true, Ordering::Relaxed);
    }
    pub fn take_text_changed(&self) -> bool {
        self.changes.text.swap(false, Ordering::Relaxed)
    }
    pub fn take_pasted(&self) -> bool {
        self.pasted.replace(false)
    }
    pub fn note_pasted(&self) {
        self.pasted.set(true);
    }
    pub fn is_read_only(&self) -> bool {
        self.display.borrow().read_only
    }
    pub fn is_plain(&self) -> bool {
        self.display.borrow().plain
    }
    pub fn is_composing(&self) -> bool {
        self.session.composing()
    }
    pub fn set_read_only(&self, value: bool) {
        self.overlays
            .lock()
            .expect("editor overlay lock poisoned")
            .read_only = value;
        self.display.borrow_mut().read_only = value;
        self.session.set_editable(!value);
        self.invalidate_display();
    }
    pub fn set_plain_text_mode(&self, plain: bool) -> Result<()> {
        if plain == self.is_plain() {
            return Ok(());
        }
        let text = self.text()?;
        let modified = self.is_modified();
        let buffered = self.has_buffered_changes();
        let manager = manager(plain, self.display.borrow().auto_links)?;
        let doc = PlainTextFormat::default().parse(manager.schema(), &text)?;
        *self.manager.borrow_mut() = manager;
        self.display.borrow_mut().plain = plain;
        self.load(doc)?;
        self.changes.modified.store(modified, Ordering::Relaxed);
        self.changes.buffered.store(buffered, Ordering::Relaxed);
        Ok(())
    }
    fn editable(&self) -> Result<()> {
        ensure!(!self.is_read_only(), "the document is locked");
        Ok(())
    }
    fn run(&self, command: &BoxCommand) -> Result<()> {
        self.editable()?;
        ensure!(
            self.session.run(command),
            "this command does not apply at the current selection"
        );
        Ok(())
    }
    pub fn run_command(&self, id: &str, params: &Value) -> Result<Value> {
        let id = match id {
            "edit.undo" => "undo",
            "edit.redo" => "redo",
            other => other,
        };
        let command = self
            .manager
            .borrow()
            .commands()
            .make(id, params, self.manager.borrow().schema())
            .map_err(anyhow::Error::msg)?;
        self.run(&command)?;
        Ok(Value::Bool(true))
    }
    pub fn replace_current(&self, text: &str) -> Result<()> {
        self.editable()?;
        validate_text(text)?;
        ensure!(
            self.text()?.len().saturating_add(text.len()) <= MAX_DOCUMENT_BYTES,
            "the replacement is too large"
        );
        let text = normalize_newlines(text);
        let state = self.session.state();
        let slice = slice_from_text(state.schema(), &text, state.selection().resolved_from());
        let mut tr = state.tr();
        tr.replace_selection(slice).scroll_into_view();
        self.session.dispatch(tr)?;
        Ok(())
    }
    pub fn replace_ime(&self, range: Range<usize>, text: &str, _join_previous: bool) -> Result<()> {
        self.select_range(range)?;
        self.replace_current(text)
    }
    pub fn insert_rtf(&self, bytes: &[u8]) -> Result<()> {
        validate_rtf(bytes)?;
        self.paste_document(
            wordcraft_formats::rtf::import(bytes.trim_ascii_start()).map_err(anyhow::Error::msg)?,
        )
    }
    pub fn insert_html(&self, html: &str) -> Result<()> {
        self.editable()?;
        ensure!(!self.is_plain(), "formatted HTML requires rich-text mode");
        crate::formats::validate_clipboard_html(html)?;
        let html = crate::formats::safe_native_html(html)?;
        let state = self.session.state();
        let slice = DomParser::from_schema(state.schema())
            .parse_with_context(&html, state.selection().resolved_from());
        let mut tr = state.tr();
        tr.replace_selection(slice).scroll_into_view();
        self.session.dispatch(tr)?;
        self.note_pasted();
        Ok(())
    }
    pub fn paste_document(&self, document: Document) -> Result<()> {
        self.insert_html(&wordcraft_formats::html::export(&document))
    }
    pub fn paste_html(&self) -> Result<()> {
        clipboard::paste_html(self)
    }
    pub fn selected_clipboard(&self) -> Result<(String, String, Vec<u8>, Slice)> {
        clipboard::selection_content(&self.session.state())
    }
    pub fn paste_slice(&self, slice: Slice) -> Result<()> {
        self.editable()?;
        let state = self.session.state();
        let content = Fragment::from_json(state.schema(), &slice.content.to_json())?;
        let slice = Slice::new(content, slice.open_start, slice.open_end);
        let mut tr = state.tr();
        tr.replace_selection(slice).scroll_into_view();
        self.session.dispatch(tr)?;
        self.note_pasted();
        Ok(())
    }
    pub fn edit_lines(&self, edit: crate::editing::TextEdit) -> Result<()> {
        self.editable()?;
        let state = self.session.state();
        let map = self.map();
        let mut lines: Vec<Vec<Node>> = vec![Vec::new()];
        for piece in &edit.pieces {
            let (text, mut offset, original) = match piece {
                crate::editing::EditPiece::Text(text) => (text.as_str(), 0, false),
                crate::editing::EditPiece::Original(range) => {
                    (map.slice(range.clone())?, range.start, true)
                }
            };
            for ch in text.chars() {
                if ch == '\n' {
                    lines.push(Vec::new());
                } else {
                    let marks = if original {
                        state.doc().resolve(map.position(offset)?)?.marks().to_vec()
                    } else {
                        state.selection().resolved_from().marks().to_vec()
                    };
                    lines
                        .last_mut()
                        .expect("an edit starts with a line")
                        .push(state.schema().text(&ch.to_string(), marks));
                }
                offset += ch.len_utf16();
            }
        }
        let paragraphs = lines
            .into_iter()
            .map(|nodes| {
                state
                    .schema()
                    .node("paragraph", None, Fragment::from(nodes), Vec::new())
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.select_range(edit.range)?;
        let mut tr = self.session.state().tr();
        tr.replace_selection(Slice::new(Fragment::from(paragraphs), 1, 1));
        self.session.dispatch(tr)?;
        self.select_range(edit.selection)
    }

    fn matches(
        &self,
        needle: &str,
        match_case: bool,
        whole_word: bool,
    ) -> Result<Vec<Range<usize>>> {
        ensure!(
            !needle.contains('\0'),
            "the search text contains a NUL character"
        );
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let text = self.text()?;
        let needle = normalize_newlines(needle);
        let mut ranges = Vec::new();
        let mut byte = 0;
        while byte < text.len() {
            let candidate = &text[byte..];
            let end = if match_case {
                candidate
                    .starts_with(&needle)
                    .then_some(byte + needle.len())
            } else {
                let mut matched = String::new();
                let wanted = needle.to_lowercase();
                let mut end = None;
                for (offset, ch) in candidate.char_indices() {
                    matched.extend(ch.to_lowercase());
                    if matched == wanted {
                        end = Some(byte + offset + ch.len_utf8());
                        break;
                    }
                    if !wanted.starts_with(&matched) {
                        break;
                    }
                }
                end
            };
            if let Some(end) = end {
                let word = |ch: char| ch.is_alphanumeric() || ch == '_';
                if !whole_word
                    || (!text[..byte].chars().next_back().is_some_and(word)
                        && !text[end..].chars().next().is_some_and(word))
                {
                    ranges.push(
                        text[..byte].encode_utf16().count()..text[..end].encode_utf16().count(),
                    );
                    byte = end;
                    continue;
                }
            }
            byte += candidate
                .chars()
                .next()
                .expect("the remaining text is nonempty")
                .len_utf8();
        }
        Ok(ranges)
    }
    pub fn find(
        &self,
        needle: &str,
        match_case: bool,
        whole_word: bool,
        backwards: bool,
    ) -> Result<bool> {
        let ranges = self.matches(needle, match_case, whole_word)?;
        let old = self.selection();
        let found = if backwards {
            ranges
                .iter()
                .rev()
                .find(|r| r.end <= old.start)
                .or(ranges.last())
        } else {
            ranges
                .iter()
                .find(|r| r.start >= old.end)
                .or(ranges.first())
        };
        if let Some(range) = found {
            self.select_range(range.clone())?;
            self.scroll_to_selection();
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn replace_all(
        &self,
        needle: &str,
        replacement: &str,
        match_case: bool,
        whole_word: bool,
    ) -> Result<usize> {
        self.editable()?;
        validate_text(replacement)?;
        let ranges = self.matches(needle, match_case, whole_word)?;
        let state = self.session.state();
        let map = self.map();
        let mut tr = state.tr();
        for range in ranges.iter().rev() {
            let from = map.position(range.start)?;
            let to = map.position(range.end)?;
            tr.set_selection(TextSelection::create(tr.doc(), from, Some(to)));
            let slice = slice_from_text(
                state.schema(),
                &normalize_newlines(replacement),
                tr.selection().resolved_from(),
            );
            tr.replace_selection(slice);
        }
        if !ranges.is_empty() {
            self.session.dispatch(tr)?;
        }
        Ok(ranges.len())
    }
    pub fn execute(&self, command: EditCommand) -> Result<()> {
        use EditCommand as E;
        let (id, args) = match command {
            E::Copy => return clipboard::copy(self, false),
            E::Cut => return clipboard::copy(self, true),
            E::Paste => return clipboard::paste(self, self.is_plain()),
            E::PastePlain => return clipboard::paste(self, true),
            E::SelectAll => {
                let command = self
                    .manager
                    .borrow()
                    .commands()
                    .make("selectAll", &Value::Null, self.session.state().schema())
                    .map_err(anyhow::Error::msg)?;
                self.session.run(&command);
                return Ok(());
            }
            E::Delete => {
                return if self.selection().is_empty() {
                    Ok(())
                } else {
                    self.replace_current("")
                };
            }
            E::CopyStyle => {
                COPIED_STYLE.with(|s| *s.borrow_mut() = Some(self.formatting()));
                return Ok(());
            }
            E::PasteStyle => {
                let s = COPIED_STYLE
                    .with(|s| s.borrow().clone())
                    .context("Copy a style first with Format > Font > Copy Style")?;
                for c in [
                    E::Font(s.font),
                    E::FontSize(s.size),
                    E::Foreground(s.foreground),
                    E::Background(s.background),
                ] {
                    self.execute(c)?;
                }
                for (mark, on) in [
                    ("bold", s.bold),
                    ("italic", s.italic),
                    ("underline", s.underline),
                    ("strike", s.strikethrough),
                ] {
                    if self.session.is_active(Some(mark), None) != on {
                        self.toggle_mark(mark)?;
                    }
                }
                return Ok(());
            }
            E::Undo => ("undo", json!({})),
            E::Redo => ("redo", json!({})),
            E::Bold => return self.toggle_mark("bold"),
            E::Italic => return self.toggle_mark("italic"),
            E::Underline => return self.toggle_mark("underline"),
            E::Strikethrough => return self.toggle_mark("strike"),
            E::Subscript => return self.toggle_mark("subscript"),
            E::Superscript => return self.toggle_mark("superscript"),
            E::Font(name) => {
                ensure!(
                    !name.is_empty() && !name.contains('\0'),
                    "choose a valid font name"
                );
                if self.is_plain() {
                    self.plain_style.borrow_mut().font = name;
                    self.invalidate_display();
                    return Ok(());
                }
                return self.text_style("fontFamily", json!(name));
            }
            E::FontSize(size) => {
                ensure!(
                    size.is_finite() && (1.0..=1638.0).contains(&size),
                    "font size must be between 1 and 1638 points"
                );
                if self.is_plain() {
                    self.plain_style.borrow_mut().size = size;
                    self.invalidate_display();
                    return Ok(());
                }
                return self.text_style("fontSize", json!(format!("{size}pt")));
            }
            E::Foreground(color) => {
                if self.is_plain() {
                    self.plain_style.borrow_mut().foreground = color & 0xffffff;
                    self.invalidate_display();
                    return Ok(());
                }
                return self.text_style("color", json!(format!("#{:06x}", color & 0xffffff)));
            }
            E::Background(color) => {
                return self.text_style(
                    "backgroundColor",
                    json!(format!("#{:06x}", color & 0xffffff)),
                );
            }
            E::Align(a) => (
                "setTextAlign",
                json!({"alignment":match a{Alignment::Left=>"left",Alignment::Center=>"center",Alignment::Right=>"right",Alignment::Justify=>"justify"}}),
            ),
            E::List(ListKind::Bullet) => ("toggleBulletList", json!({})),
            E::List(ListKind::Number) => ("toggleOrderedList", json!({})),
            E::List(ListKind::None) => ("clearNodes", json!({})),
            E::ConvertCase(case) => {
                let selected = self.selected_text()?;
                let text = match case {
                    Case::Upper => selected.to_uppercase(),
                    Case::Lower => selected.to_lowercase(),
                    Case::Title => {
                        let mut start = true;
                        selected
                            .chars()
                            .flat_map(|c| {
                                let out = if start {
                                    c.to_uppercase().collect::<String>()
                                } else {
                                    c.to_lowercase().collect()
                                };
                                start = !c.is_alphanumeric();
                                out.chars().collect::<Vec<_>>()
                            })
                            .collect()
                    }
                };
                return self.replace_current(&text);
            }
            E::LineSpacing(lines) => {
                ensure!(
                    lines.is_finite() && (0.5..=10.).contains(&lines),
                    "line spacing must be between 0.5 and 10"
                );
                return self.paragraph_style("lineHeight", json!(lines.to_string()));
            }
            E::Indent(twips) => {
                let current = self
                    .session
                    .get_attributes("paragraph")
                    .get("indent")
                    .and_then(Value::as_str)
                    .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
                    .unwrap_or(0.);
                return self.paragraph_style(
                    "indent",
                    json!(format!(
                        "{}px",
                        (current + twips as f32 / 20.).clamp(0., 1440.)
                    )),
                );
            }
            E::Direction(rtl) => {
                return self.paragraph_style("direction", json!(if rtl { "rtl" } else { "ltr" }));
            }
            E::FirstLineIndent(_) | E::TabStop(_) => bail!(
                "First-line indentation and custom tab stops are not supported by NSH 0.5.0's native rich-text view."
            ),
        };
        self.run_command(id, &args)?;
        Ok(())
    }
    fn paragraph_style(&self, key: &str, value: Value) -> Result<()> {
        ensure!(
            !self.is_plain(),
            "paragraph styling requires rich-text mode"
        );
        let state = self.session.state();
        let mut tr = state.tr();
        let from = state.selection().from();
        let to = state.selection().to();
        let mut paragraphs = Vec::new();
        state.doc().nodes_between(from, to, &mut |node, pos, _, _| {
            if node.is_textblock() && node.ty().name() != "codeBlock" {
                let mut attrs = node.attrs().clone();
                attrs.insert(key.into(), value.clone());
                paragraphs.push((pos, attrs));
            }
            true
        });
        for (pos, attrs) in paragraphs {
            tr.set_node_markup(pos, None, Some(attrs), None)?;
        }
        self.editable()?;
        self.session.dispatch(tr)?;
        Ok(())
    }
    fn toggle_mark(&self, name: &str) -> Result<()> {
        ensure!(
            !self.is_plain(),
            "character styling requires rich-text mode"
        );
        let state = self.session.state();
        let ty = state
            .schema()
            .mark_type(name)
            .context("the native schema has no such mark")?;
        self.run(&tiptap::toggle_mark(ty, Attrs::new(), true))
    }
    fn text_style(&self, key: &str, value: Value) -> Result<()> {
        ensure!(
            !self.is_plain(),
            "character styling requires rich-text mode"
        );
        let state = self.session.state();
        let ty = state
            .schema()
            .mark_type("textStyle")
            .context("the native schema has no text styles")?;
        let mut attrs = self.session.get_attributes("textStyle");
        attrs.insert(key.into(), value);
        self.run(&tiptap::set_mark(ty, attrs))
    }
    pub fn formatting(&self) -> FormatState {
        if self.is_plain() {
            return self.plain_style.borrow().clone();
        }
        let attrs = self.session.get_attributes("textStyle");
        let para = self.session.get_attributes("paragraph");
        let color = |key: &str, fallback: u32| {
            attrs
                .get(key)
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<csscolorparser::Color>().ok())
                .map(|c| {
                    let [r, g, b, _] = c.to_rgba8();
                    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
                })
                .unwrap_or(fallback)
        };
        FormatState {
            font: attrs
                .get("fontFamily")
                .and_then(Value::as_str)
                .unwrap_or(crate::fonts::default_rich_font())
                .into(),
            size: attrs
                .get("fontSize")
                .and_then(Value::as_str)
                .and_then(|v| v.trim_end_matches("pt").trim_end_matches("px").parse().ok())
                .unwrap_or(13.),
            bold: self.session.is_active(Some("bold"), None),
            italic: self.session.is_active(Some("italic"), None),
            underline: self.session.is_active(Some("underline"), None),
            strikethrough: self.session.is_active(Some("strike"), None),
            foreground: color("color", 0),
            background: color("backgroundColor", 0xffffff),
            alignment: match para.get("textAlign").and_then(Value::as_str) {
                Some("center") => Alignment::Center,
                Some("right") => Alignment::Right,
                Some("justify") => Alignment::Justify,
                _ => Alignment::Left,
            },
        }
    }
    pub fn insert_table(&self, rows: u16, columns: u16) -> Result<()> {
        ensure!(!self.is_plain(), "tables require rich-text mode");
        ensure!(
            (1..=100).contains(&rows) && (1..=63).contains(&columns),
            "a table must have 1–100 rows and 1–63 columns"
        );
        self.run_command(
            "insertTable",
            &json!({"rows":rows,"cols":columns,"withHeaderRow":false}),
        )?;
        Ok(())
    }
    pub fn insert_link(&self, url: &str, label: &str) -> Result<()> {
        self.editable()?;
        ensure!(!self.is_plain(), "formatted links require rich-text mode");
        ensure!(
            !url.is_empty() && wordcraft_doc::link::is_safe_href(url),
            "Use a web, email, bookmark or relative link."
        );
        validate_text(label)?;
        let text = if label.is_empty() { url } else { label };
        let state = self.session.state();
        let ty = state
            .schema()
            .mark_type("link")
            .context("the schema has no links")?;
        let mut attrs = Attrs::new();
        attrs.insert("href".into(), json!(url));
        let mark = ty.create(Some(attrs))?;
        let node = state.schema().text(text, vec![mark]);
        let mut tr = state.tr();
        tr.replace_selection_with(node, false).scroll_into_view();
        self.session.dispatch(tr)?;
        Ok(())
    }
    pub fn insert_image(&self, path: &Path) -> Result<()> {
        self.editable()?;
        ensure!(!self.is_plain(), "pictures require rich-text mode");
        let meta = std::fs::metadata(path).context("reading the image")?;
        ensure!(
            meta.is_file() && meta.len() <= 64 * 1024 * 1024,
            "choose an image no larger than 64 MiB"
        );
        let bytes = std::fs::read(path).context("reading the image")?;
        self.insert_image_bytes(&bytes)
    }
    pub fn insert_image_bytes(&self, bytes: &[u8]) -> Result<()> {
        use base64::Engine as _;
        self.editable()?;
        ensure!(!self.is_plain(), "pictures require rich-text mode");
        clipboard::checked_image(bytes)?;
        let image = image::load_from_memory(bytes).context("decoding the image")?;
        let mut output = std::io::Cursor::new(Vec::new());
        image.write_to(&mut output, image::ImageFormat::Png)?;
        self.run_command("setImage",&json!({"src":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(output.into_inner())),"width":image.width(),"height":image.height()}))?;
        Ok(())
    }
    pub fn preview(&self, document: Document) -> Result<Self> {
        let preview = Self::new()?;
        preview.set_document(document)?;
        *preview.display.borrow_mut() = self.display.borrow().clone();
        preview.display.borrow_mut().plain = false;
        preview.display.borrow_mut().syntax.clear();
        preview.display.borrow_mut().proofing.clear();
        preview.set_read_only(true);
        preview.mark_saved();
        Ok(preview)
    }
    pub fn page_settings(&self) -> PageSettings {
        self.page.borrow().clone()
    }
    pub fn set_page_settings(&self, page: &PageSettings) -> Result<()> {
        self.editable()?;
        printing::validate_page_settings(page)?;
        *self.page.borrow_mut() = page.clone();
        self.mark_modified();
        self.invalidate_display();
        Ok(())
    }
    pub fn export_pdf(&self, path: &Path, page: &PageSettings) -> Result<()> {
        printing::export_pdf(&self.document_snapshot()?, path, page)
    }
    /// Opens the offline word-completion popup without changing the document.
    pub fn complete(&self) -> Result<()> {
        self.editable()?;
        let selection = self.selection();
        ensure!(
            selection.is_empty(),
            "place the caret in the word to complete"
        );
        ensure!(
            !self.is_composing(),
            "finish the input-method composition first"
        );
        self.show_completion(true)
            .context("No completion is available for this word.")
    }
    fn completion_key(&self) -> (u64, usize, usize) {
        let state = self.session.state();
        (
            self.document_revision(),
            state.selection().from(),
            state.selection().to(),
        )
    }
    fn show_completion(&self, manual: bool) -> Option<()> {
        let key = self.completion_key();
        let text = self.plain_text_snapshot();
        let language = self.display.borrow().completion_language.clone();
        let result =
            crate::completion::suggest(&text, self.selection().end, Some(&language), manual)?;
        *self.completion.borrow_mut() = Some(CompletionPopup {
            result,
            selected: 0,
            key,
        });
        self.completion_seen.set(Some(key));
        self.invalidate_display();
        Some(())
    }
    pub(super) fn refresh_completion(&self) {
        let key = self.completion_key();
        if !self.display.borrow().visible
            || self.is_read_only()
            || self.is_composing()
            || key.1 != key.2
        {
            self.dismiss_completion();
            return;
        }
        if self
            .completion
            .borrow()
            .as_ref()
            .is_some_and(|popup| popup.key == key)
        {
            return;
        }
        if self.completion.borrow_mut().take().is_some() {
            self.invalidate_display();
        }
        if !self.display.borrow().autocomplete_enabled
            || !self.session.is_focused()
            || self.completion_seen.get() == Some(key)
        {
            return;
        }
        self.completion_seen.set(Some(key));
        let _ = self.show_completion(false);
    }
    pub(super) fn completion_state(&self) -> Option<(crate::completion::Completion, usize)> {
        self.completion
            .borrow()
            .as_ref()
            .map(|popup| (popup.result.clone(), popup.selected))
    }
    pub(super) fn move_completion(&self, delta: i32) {
        let mut popup = self.completion.borrow_mut();
        if let Some(popup) = popup.as_mut() {
            popup.selected = (popup.selected as i32 + delta)
                .rem_euclid(popup.result.candidates.len() as i32)
                as usize;
            self.invalidate_display();
        }
    }
    pub(super) fn dismiss_completion(&self) {
        if self.completion.borrow_mut().take().is_some() {
            self.completion_seen.set(Some(self.completion_key()));
            self.invalidate_display();
        }
    }
    pub(super) fn accept_completion(&self, index: Option<usize>) -> Result<()> {
        let popup = self
            .completion
            .borrow_mut()
            .take()
            .context("No completion is selected.")?;
        self.invalidate_display();
        ensure!(
            popup.key == self.completion_key(),
            "The word changed before the completion was chosen."
        );
        let word = popup
            .result
            .candidates
            .get(index.unwrap_or(popup.selected))
            .context("The selected completion is unavailable.")?;
        self.select_range(popup.result.range)?;
        self.replace_current(word)?;
        self.completion_seen.set(Some(self.completion_key()));
        self.focus();
        Ok(())
    }
    pub fn set_visible(&self, visible: bool) {
        let value = visible;
        if self.display.borrow().visible != value {
            self.display.borrow_mut().visible = value;
            self.invalidate_display();
        }
    }
    pub fn focus(&self) {
        if self.display.borrow().visible {
            self.focus_requested.set(true);
            self.session.focus();
        }
    }
    pub fn restore_focus(&self) {
        self.focus();
    }
    pub fn take_focus_request(&self) -> bool {
        self.focus_requested.replace(false)
    }
    pub fn scroll_to_selection(&self) {
        self.scroll_requested.set(true);
        let command = tiptap::scroll_into_view();
        self.session.run(&command);
    }
    pub fn take_scroll_request(&self) -> bool {
        self.scroll_requested.replace(false)
    }
    pub fn scroll_editor(&self, amount: i32, page: bool) {
        let old = self.scroll_delta.get();
        self.scroll_delta.set((old.0.saturating_add(amount), page));
    }
    pub fn take_scroll_delta(&self) -> (i32, bool) {
        self.scroll_delta.replace((0, false))
    }
    pub fn view_scroll(&self) -> (f32, f32) {
        self.view_scroll.get()
    }
    pub fn set_view_scroll(&self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.view_scroll.set((x.max(0.), y.max(0.)));
        }
    }
    pub fn enqueue_shortcut(&self, shortcut: &str) {
        self.shortcuts.borrow_mut().push_back(shortcut.into());
    }
    pub fn take_shortcut(&self) -> Option<String> {
        self.shortcuts.borrow_mut().pop_front()
    }
    pub fn display_state(&self) -> EditorDisplayState {
        let mut s = self.display.borrow().clone();
        s.revision = self.revision();
        s
    }
    pub fn set_zoom(&self, percent: u16) {
        let value = percent.clamp(10, 500);
        if self.display.borrow().zoom != value {
            self.display.borrow_mut().zoom = value;
            self.invalidate_display();
        }
    }
    pub fn set_wrap(&self, wrap: bool) {
        let value = wrap;
        if self.display.borrow().wrap != value {
            self.display.borrow_mut().wrap = value;
            self.invalidate_display();
        }
    }
    pub fn set_wrap_to_page(&self, value: bool) {
        let value = value;
        if self.display.borrow().wrap_to_page != value {
            self.display.borrow_mut().wrap_to_page = value;
            self.invalidate_display();
        }
    }
    pub fn set_background(&self, color: u32) {
        let value = color & 0xffffff;
        if self.display.borrow().background != value {
            self.display.borrow_mut().background = value;
            self.invalidate_display();
        }
    }
    pub fn set_text_override(&self, color: Option<u32>) -> Result<()> {
        self.display.borrow_mut().text_override = color.map(|c| c & 0xffffff);
        self.invalidate_display();
        Ok(())
    }
    pub fn set_auto_links(&self, enabled: bool) {
        if self.display.borrow().auto_links == enabled {
            return;
        }
        self.display.borrow_mut().auto_links = enabled;
        let state = self.session.state();
        match state.reconfigure(plugins(
            &self.manager.borrow(),
            self.overlays.clone(),
            self.is_plain(),
            enabled,
        )) {
            Ok(state) => self.session.update_state(state),
            Err(error) => self.report_error(error.to_string()),
        }
        self.invalidate_display();
    }
    pub fn set_autocomplete_enabled(&self, value: bool) {
        let value = value;
        if self.display.borrow().autocomplete_enabled != value {
            self.display.borrow_mut().autocomplete_enabled = value;
            self.invalidate_display();
        }
    }
    pub fn set_completion_language(&self, value: Option<String>) {
        self.display.borrow_mut().completion_language = value.unwrap_or_default();
        self.invalidate_display();
    }
    pub fn set_viewport_width(&self, width: f32) {
        if width.is_finite() && width > 0. {
            self.viewport_width.set(width);
            self.invalidate_display();
        }
    }
    pub fn apply_spans(&self, spans: &[crate::syntax::Span]) -> Result<()> {
        ensure!(
            self.is_plain() || spans.is_empty(),
            "source coloring requires plain-text mode"
        );
        let map = self.map();
        for span in spans {
            map.slice(
                span.start
                    ..span
                        .start
                        .checked_add(span.len)
                        .context("syntax span overflow")?,
            )?;
        }
        if self.display.borrow().syntax != spans
            || self
                .overlays
                .lock()
                .expect("editor overlay lock poisoned")
                .doc
                .as_ref()
                != Some(self.session.state().doc())
        {
            self.display.borrow_mut().syntax = spans.to_vec();
            let mut overlays = self.overlays.lock().expect("editor overlay lock poisoned");
            overlays.doc = Some(self.session.state().doc().clone());
            overlays.syntax = spans.to_vec();
            drop(overlays);
            self.session.update_state(self.session.state());
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn clear_spans(&self) -> Result<()> {
        self.apply_spans(&[])
    }
    pub fn set_proofing_marks(&self, ranges: &[Range<usize>]) -> Result<()> {
        let map = self.map();
        for range in ranges {
            map.slice(range.clone())?;
        }
        if self.display.borrow().proofing != ranges
            || self
                .overlays
                .lock()
                .expect("editor overlay lock poisoned")
                .doc
                .as_ref()
                != Some(self.session.state().doc())
        {
            self.display.borrow_mut().proofing = ranges.to_vec();
            let mut overlays = self.overlays.lock().expect("editor overlay lock poisoned");
            overlays.doc = Some(self.session.state().doc().clone());
            overlays.proofing = ranges.to_vec();
            drop(overlays);
            self.session.update_state(self.session.state());
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn adapt_automatic_text(&self) -> Result<()> {
        self.invalidate_display();
        Ok(())
    }
}
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
fn validate_text(text: &str) -> Result<()> {
    ensure!(
        !text.contains('\0'),
        "text contains an embedded NUL character"
    );
    ensure!(
        text.len() <= MAX_DOCUMENT_BYTES,
        "the document is too large"
    );
    Ok(())
}
fn validate_rtf(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_DOCUMENT_BYTES,
        "the rich-text document is too large"
    );
    ensure!(
        bytes.trim_ascii_start().starts_with(b"{\\rtf"),
        "the document is not valid RTF"
    );
    Ok(())
}
#[cfg(test)]
mod core_tests;

fn validate_native_resources(doc: &Node) -> Result<()> {
    let mut valid = true;
    doc.descendants(&mut |node, _, _, _| {
        if node.ty().name() == "image" {
            let src = node
                .attrs()
                .get("src")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !src.is_empty() && !src.starts_with("data:image/") {
                valid = false;
            }
        }
        true
    });
    ensure!(
        valid,
        "Document images must contain embedded image data; remote and filesystem image URLs are not opened."
    );
    Ok(())
}
