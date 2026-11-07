//! Toolkit-independent WordCraft editing session. Public ranges count UTF-16 code units.
use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    collections::VecDeque,
    ops::Range,
    path::Path,
    sync::Arc,
};
use wordcraft_doc::{
    CharProps, Document, Pos, StoryRef,
    props::{Align, Rgb, TextColor},
};
use wordcraft_engine::{Selection, Session};
use wordcraft_layout::{DocLayout, LayoutCache, LayoutOptions, ViewMode};

pub mod clipboard;
mod commands;
mod core_positions;
pub mod printing;
pub mod surface;
mod types;
pub use types::{
    Alignment, Case, EditCommand, EditorDisplayState, FormatState, ListKind, PageSettings,
};

const MAX_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
thread_local! { static COPIED_STYLE: RefCell<Option<CharProps>> = const { RefCell::new(None) }; }
struct DisplayDocument {
    key: u64,
    document: Document,
    layout: Arc<DocLayout>,
}

/// A document stays independent of any window, so moving tabs preserves its history and selection.
pub struct Editor {
    session: RefCell<Session>,
    display: RefCell<EditorDisplayState>,
    display_revision: Cell<u64>,
    mapped: RefCell<Option<(u64, core_positions::TextMap)>>,
    rendered: RefCell<Option<DisplayDocument>>,
    layout_cache: RefCell<LayoutCache>,
    modified: Cell<bool>,
    buffered_changes: Cell<bool>,
    text_changed: Cell<bool>,
    pasted: Cell<bool>,
    focus_requested: Cell<bool>,
    scroll_requested: Cell<bool>,
    scroll_delta: Cell<(i32, bool)>,
    view_scroll: Cell<(f32, f32)>,
    viewport_width: Cell<f32>,
    shortcuts: RefCell<VecDeque<String>>,
}
impl Editor {
    pub fn new() -> Result<Self> {
        crate::fonts::register_engine_fonts()?;
        let mut session = Session::new(Document::from_text(""));
        commands::install(&mut session);
        session.view.proofing = false;
        let ac = &mut session.prefs.autocorrect;
        ac.replace_text = false;
        ac.two_initial_caps = false;
        ac.cap_sentences = false;
        ac.cap_cells = false;
        ac.cap_days = false;
        ac.caps_lock = false;
        ac.smart_quotes = false;
        ac.fractions = false;
        ac.ordinals = false;
        ac.bold_italic = false;
        ac.dashes = false;
        ac.bullets = false;
        ac.numbering = false;
        ac.border_lines = false;
        ac.tab_indent = false;
        ac.heading_styles = false;
        ac.double_space_period = false;
        Ok(Self {
            session: RefCell::new(session),
            display: RefCell::new(EditorDisplayState::default()),
            display_revision: Cell::new(1),
            mapped: RefCell::new(None),
            rendered: RefCell::new(None),
            layout_cache: RefCell::new(LayoutCache::new()),
            modified: Cell::new(false),
            buffered_changes: Cell::new(false),
            text_changed: Cell::new(false),
            pasted: Cell::new(false),
            focus_requested: Cell::new(false),
            scroll_requested: Cell::new(false),
            scroll_delta: Cell::new((0, false)),
            view_scroll: Cell::new((0.0, 0.0)),
            viewport_width: Cell::new(600.0),
            shortcuts: RefCell::new(VecDeque::new()),
        })
    }
    pub fn session(&self) -> Ref<'_, Session> {
        self.session.borrow()
    }
    /// Use run_command for edits; this borrow is for transient selection/view state only.
    pub fn session_mut(&self) -> RefMut<'_, Session> {
        self.session.borrow_mut()
    }
    pub fn snapshot(&self) -> Document {
        self.session.borrow().doc.clone()
    }
    pub fn revision(&self) -> u64 {
        self.session
            .borrow()
            .rev()
            .wrapping_mul(1_000_003)
            .wrapping_add(self.display_revision.get())
    }
    fn invalidate_display(&self) {
        self.display_revision
            .set(self.display_revision.get().wrapping_add(1));
    }
    fn changed(&self) {
        self.modified.set(true);
        self.buffered_changes.set(true);
        self.text_changed.set(true);
        self.scroll_requested.set(true);
    }
    pub fn run_command(&self, id: &str, params: &Value) -> Result<Value> {
        let mut session = self.session.borrow_mut();
        let spec = session
            .registry
            .get(id)
            .with_context(|| format!("unknown editor command: {id}"))?;
        ensure!(
            !self.is_read_only() || !spec.mutates,
            "the document is locked"
        );
        let mutates = spec.mutates;
        let before = session.doc.clone();
        let pending = session.pending.clone();
        let result = session.run(id, params).map_err(anyhow::Error::new)?;
        if !before.same_content(&session.doc) || (mutates && pending != session.pending) {
            self.changed();
        }
        Ok(result)
    }
    fn with_map<T>(&self, f: impl FnOnce(&core_positions::TextMap) -> T) -> T {
        let session = self.session.borrow();
        let rev = session.rev();
        let mut cached = self.mapped.borrow_mut();
        if cached.as_ref().is_none_or(|(r, _)| *r != rev) {
            *cached = Some((rev, core_positions::TextMap::new(&session.doc)));
        }
        f(&cached.as_ref().expect("the document map was initialized").1)
    }
    pub fn text(&self) -> Result<String> {
        self.with_map(|m| {
            ensure!(
                m.text.len() <= MAX_DOCUMENT_BYTES,
                "the document exceeds the supported text size"
            );
            Ok(m.text.clone())
        })
    }
    pub fn plain_text_snapshot(&self) -> String {
        self.with_map(|m| m.text.clone())
    }
    pub fn text_length(&self) -> usize {
        self.with_map(|m| m.len())
    }
    pub fn position_at_utf16(&self, offset: usize) -> Result<Pos> {
        self.with_map(|m| m.position(offset))
    }
    pub fn utf16_at_position(&self, position: &Pos) -> Option<usize> {
        self.with_map(|m| m.offset(position))
    }
    pub fn selection(&self) -> Range<usize> {
        let (anchor, focus) = {
            let session = self.session.borrow();
            (session.sel.anchor.clone(), session.sel.focus.clone())
        };
        self.with_map(|m| {
            let a = m.offset(&anchor).unwrap_or(0);
            let b = m.offset(&focus).unwrap_or(a);
            a.min(b)..a.max(b)
        })
    }
    pub fn select_range(&self, range: Range<usize>) -> Result<()> {
        ensure!(
            range.start <= range.end,
            "the requested selection is reversed"
        );
        let (a, b) = self.with_map(|m| {
            Ok::<_, anyhow::Error>((m.position(range.start)?, m.position(range.end)?))
        })?;
        self.set_document_selection(a, b)
    }
    pub fn set_document_selection(&self, anchor: Pos, focus: Pos) -> Result<()> {
        let mut session = self.session.borrow_mut();
        ensure!(
            anchor.story == focus.story,
            "a selection must stay within one document story"
        );
        ensure!(
            session.doc.clamp(&anchor) == anchor && session.doc.clamp(&focus) == focus,
            "the selection is outside the document"
        );
        if session.sel.anchor != anchor || session.sel.focus != focus {
            session.close_typing();
            session.pending = None;
            session.column = None;
            session.goal_x = None;
            session.sel = Selection { anchor, focus };
        }
        Ok(())
    }
    pub fn selected_text(&self) -> Result<String> {
        let range = self.selection();
        self.with_map(|m| Ok(m.slice(range)?.to_owned()))
    }
    pub fn set_document(&self, mut document: Document) -> Result<()> {
        document.ensure_nonempty();
        ensure!(
            document.plain_text(StoryRef::Body).len() <= MAX_DOCUMENT_BYTES,
            "the document is too large"
        );
        self.session.borrow_mut().set_document(document);
        *self.mapped.borrow_mut() = None;
        let mut display = self.display.borrow_mut();
        display.syntax.clear();
        display.proofing.clear();
        drop(display);
        self.modified.set(false);
        self.buffered_changes.set(false);
        self.text_changed.set(true);
        self.view_scroll.set((0.0, 0.0));
        self.scroll_requested.set(true);
        self.invalidate_display();
        Ok(())
    }
    pub fn set_text(&self, text: &str) -> Result<()> {
        validate_text(text)?;
        let text = normalize_newlines(text);
        let font = self.formatting();
        let mut document = Document::from_text(&text);
        if self.is_plain() {
            let defaults = &mut Arc::make_mut(&mut document.styles).default_chr;
            defaults.font = Some(font.font);
            defaults.size = Some(font.size.max(1.0));
        }
        self.set_document(document)
    }
    pub fn rtf(&self) -> Result<Vec<u8>> {
        let bytes = wordcraft_formats::rtf::export(&self.session.borrow().doc).into_bytes();
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_DOCUMENT_BYTES,
            "the serialized rich-text document is too large"
        );
        Ok(bytes)
    }
    pub fn set_rtf(&self, bytes: &[u8]) -> Result<()> {
        let bytes = bytes.trim_ascii_start();
        commands::validate_rtf(bytes)?;
        let document = wordcraft_formats::rtf::import(bytes).map_err(anyhow::Error::msg)?;
        self.set_document(document)
    }
    pub fn is_modified(&self) -> bool {
        self.modified.get()
    }
    pub fn has_buffered_changes(&self) -> bool {
        self.buffered_changes.get()
    }
    pub fn mark_saved(&self) {
        self.modified.set(false);
        self.buffered_changes.set(false);
        self.session.borrow_mut().stamp_save();
    }
    pub fn acknowledge_buffered_changes(&self) {
        self.buffered_changes.set(false);
    }
    pub fn mark_modified(&self) {
        self.modified.set(true);
        self.buffered_changes.set(true);
    }
    pub fn take_text_changed(&self) -> bool {
        self.text_changed.replace(false)
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
    pub fn set_read_only(&self, value: bool) {
        if self.display.borrow().read_only != value {
            self.display.borrow_mut().read_only = value;
            self.invalidate_display();
        }
    }
    pub fn set_plain_text_mode(&self, plain: bool) -> Result<()> {
        if plain == self.is_plain() {
            return Ok(());
        }
        if plain {
            let text = self.text()?;
            let style = self.formatting();
            let modified = self.modified.get();
            let buffered = self.buffered_changes.get();
            let mut doc = Document::from_text(&text);
            let defaults = &mut Arc::make_mut(&mut doc.styles).default_chr;
            defaults.font = Some(style.font);
            defaults.size = Some(style.size);
            self.set_document(doc)?;
            self.modified.set(modified);
            self.buffered_changes.set(buffered);
        }
        self.display.borrow_mut().plain = plain;
        self.session.borrow_mut().prefs.autocorrect.links =
            self.display.borrow().auto_links && !plain;
        self.invalidate_display();
        Ok(())
    }
    pub fn replace_current(&self, text: &str) -> Result<()> {
        validate_text(text)?;
        let text = normalize_newlines(text);
        ensure!(
            self.text()?.len().saturating_add(text.len()) <= MAX_DOCUMENT_BYTES,
            "the replacement is too large"
        );
        if text.contains('\n') {
            self.run_command("exeditor.replace", &json!({"text": text}))?;
        } else {
            self.run_command(
                "text.insert",
                &json!({"text": text, "raw": !self.display.borrow().auto_links || self.is_plain()}),
            )?;
        }
        Ok(())
    }
    pub fn replace_ime(&self, range: Range<usize>, text: &str, join_previous: bool) -> Result<()> {
        ensure!(!self.is_read_only(), "the document is locked");
        validate_text(text)?;
        let text = normalize_newlines(text);
        self.select_range(range)?;
        let mut session = self.session.borrow_mut();
        if join_previous {
            session.join_next_undo();
        }
        let before = session.doc.clone();
        session.run_ime(
            "text.insert",
            &json!({"text": text, "raw": true, "paragraphs": text.contains('\n')}),
        )?;
        if !before.same_content(&session.doc) {
            self.changed();
        }
        Ok(())
    }
    pub fn insert_rtf(&self, bytes: &[u8]) -> Result<()> {
        let bytes = bytes.trim_ascii_start();
        ensure!(
            !self.is_plain(),
            "formatted content requires rich-text mode"
        );
        commands::validate_rtf(bytes)?;
        let doc = wordcraft_formats::rtf::import(bytes).map_err(anyhow::Error::msg)?;
        self.paste_document(doc)
    }
    pub fn insert_html(&self, html: &str) -> Result<()> {
        ensure!(!self.is_plain(), "formatted HTML requires rich-text mode");
        self.paste_document(crate::formats::clipboard_document(html)?)
    }
    pub fn paste_document(&self, document: Document) -> Result<()> {
        ensure!(
            !self.is_plain(),
            "formatted content requires rich-text mode"
        );
        ensure!(
            self.text()?
                .len()
                .saturating_add(document.plain_text(StoryRef::Body).len())
                <= MAX_DOCUMENT_BYTES,
            "the formatted paste is too large"
        );
        self.run_command("exeditor.pasteDocument", &json!({"document": document}))?;
        self.note_pasted();
        Ok(())
    }
    pub fn paste_html(&self) -> Result<()> {
        clipboard::paste_html(self)
    }
    pub fn edit_lines(&self, edit: crate::editing::TextEdit) -> Result<()> {
        let pieces: Vec<Value> = edit
            .pieces
            .into_iter()
            .map(|piece| match piece {
                crate::editing::EditPiece::Original(r) => json!({"start": r.start, "end": r.end}),
                crate::editing::EditPiece::Text(text) => json!({"text": text}),
            })
            .collect();
        self.run_command(
            "exeditor.editLines",
            &json!({
                "range": {"start": edit.range.start, "end": edit.range.end},
                "selection": {"start": edit.selection.start, "end": edit.selection.end},
                "replacement": edit.replacement, "pieces": pieces,
            }),
        )?;
        Ok(())
    }
    pub fn find(
        &self,
        needle: &str,
        match_case: bool,
        whole_word: bool,
        backwards: bool,
    ) -> Result<bool> {
        ensure!(
            !needle.contains('\0'),
            "the search text contains a NUL character"
        );
        if needle.is_empty() {
            return Ok(false);
        }
        let old = self.selection();
        let result = self.run_command(
            "edit.advancedFind",
            &json!({"text": normalize_newlines(needle), "matchCase": match_case,
            "wholeWord": whole_word, "special": false, "regex": false, "in": "main"}),
        )?;
        if result["count"].as_u64().unwrap_or(0) == 0 {
            return Ok(false);
        }
        let matches = self.session.borrow().find.results.clone();
        let ranges: Vec<_> = matches
            .iter()
            .filter_map(|(a, b)| {
                Some((self.utf16_at_position(a)?, self.utf16_at_position(b)?, a, b))
            })
            .collect();
        let found = if backwards {
            ranges
                .iter()
                .rev()
                .find(|(_, b, _, _)| *b <= old.start)
                .or(ranges.last())
        } else {
            ranges
                .iter()
                .find(|(a, _, _, _)| *a >= old.end)
                .or(ranges.first())
        };
        if let Some((_, _, a, b)) = found {
            self.set_document_selection((*a).clone(), (*b).clone())?;
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
        ensure!(!needle.contains('\0'), "find text contains a NUL character");
        validate_text(replacement)?;
        if needle.is_empty() {
            return Ok(0);
        }
        let old = self.selection();
        let result = self.run_command(
            "edit.replaceAll",
            &json!({"text": normalize_newlines(needle),
            "with": normalize_newlines(replacement).replace('\n', "\r"), "matchCase": match_case,
            "wholeWord": whole_word, "special": false, "regex": false}),
        )?;
        let count = result["replaced"].as_u64().unwrap_or(0) as usize;
        let text = self.text()?;
        let length = text.encode_utf16().count();
        let boundary = |mut n: usize| {
            n = n.min(length);
            while core_positions::utf16_to_byte(&text, n).is_none() {
                n -= 1;
            }
            n
        };
        let _ = self.select_range(boundary(old.start)..boundary(old.end));
        Ok(count)
    }
    pub fn execute(&self, command: EditCommand) -> Result<()> {
        use EditCommand as E;
        let (id, params) = match command {
            E::Copy => return clipboard::copy(self, false),
            E::Cut => return clipboard::copy(self, true),
            E::Paste => return clipboard::paste(self, self.is_plain()),
            E::PastePlain => return clipboard::paste(self, true),
            E::SelectAll => return self.select_range(0..self.text_length()),
            E::CopyStyle => {
                let style = self.resolved_style();
                COPIED_STYLE.with(|s| *s.borrow_mut() = Some(style));
                return Ok(());
            }
            E::PasteStyle => {
                let props = COPIED_STYLE
                    .with(|s| s.borrow().clone())
                    .context("Copy a style first with Format > Font > Copy Style")?;
                ("format.set", json!({"props": props}))
            }
            E::Undo => ("edit.undo", json!({})),
            E::Redo => ("edit.redo", json!({})),
            E::Delete => {
                if self.selection().is_empty() {
                    return Ok(());
                }
                return self.replace_current("");
            }
            E::Bold => ("format.bold", json!({})),
            E::Italic => ("format.italic", json!({})),
            E::Underline => ("format.underline", json!({})),
            E::Strikethrough => ("format.strikethrough", json!({})),
            E::Superscript => ("format.superscript", json!({})),
            E::Subscript => ("format.subscript", json!({})),
            E::Font(name) => {
                ensure!(
                    !name.is_empty() && !name.contains('\0'),
                    "choose a valid font name"
                );
                ("format.font", json!({"name": name}))
            }
            E::FontSize(size) => {
                ensure!(
                    size.is_finite() && (1.0..=1638.0).contains(&size),
                    "font size must be between 1 and 1638 points"
                );
                ("format.size", json!({"size": size}))
            }
            E::Foreground(color) => (
                "format.color",
                json!({"color": format!("{:06X}",color & 0xffffff)}),
            ),
            E::Background(color) => (
                "format.shading",
                json!({"color": format!("{:06X}",color & 0xffffff)}),
            ),
            E::Align(a) => (
                "para.align",
                json!({"value": match a { Alignment::Left=>"left",Alignment::Center=>"center",Alignment::Right=>"right",Alignment::Justify=>"justify" }}),
            ),
            E::LineSpacing(lines) => {
                ensure!(
                    lines.is_finite() && (0.5..=10.0).contains(&lines),
                    "line spacing must be between 0.5 and 10"
                );
                ("para.lineSpacing", json!({"value": lines}))
            }
            E::Indent(delta) => {
                let session = self.session.borrow();
                let p = session
                    .doc
                    .para_at(&session.sel.focus)
                    .context("the paragraph is missing")?;
                let resolved = session.doc.styles.resolve_para(&p.props);
                (
                    "para.indents",
                    json!({"left": (resolved.indent_left + delta as f32/20.0).clamp(0.0,1440.0)}),
                )
            }
            E::FirstLineIndent(twips) => {
                let session = self.session.borrow();
                let left = wordcraft_engine::cmd::para::selection_para(&session).indent_left;
                (
                    "para.indents",
                    json!({"firstLine":twips.clamp(0,28800) as f32/20.0-left}),
                )
            }
            E::TabStop(twips) => {
                ensure!(
                    (0..=28800).contains(&twips),
                    "the tab stop must fall within the document width"
                );
                ("para.tabs", json!({"set": [{"pos": twips as f32/20.0}]}))
            }
            E::Direction(rtl) => ("para.rtl", json!({"value": rtl})),
            E::List(kind) => match kind {
                ListKind::None => ("para.bullets", json!({"off":true})),
                ListKind::Bullet => ("para.bullets", json!({})),
                ListKind::Number => ("para.numbering", json!({})),
            },
            E::ConvertCase(case) => {
                if self.selection().is_empty() {
                    return Ok(());
                }
                (
                    "format.changeCase",
                    json!({"mode": match case { Case::Upper=>"upper",Case::Lower=>"lower",Case::Title=>"title" }}),
                )
            }
        };
        if self.is_plain() && id.starts_with("format.") && id != "format.changeCase" {
            self.run_command(
                "exeditor.plainFormat",
                &json!({"command": id, "params": params}),
            )?;
        } else {
            self.run_command(id, &params)?;
        }
        Ok(())
    }
    fn resolved_style(&self) -> CharProps {
        let s = self.session.borrow();
        let props = s.caret_props();
        let paragraph_style = s
            .doc
            .para_at(&s.sel.focus)
            .and_then(|p| p.props.style.as_deref());
        let r = s.doc.styles.resolve_char(paragraph_style, &props);
        CharProps {
            font: Some(r.font),
            size: Some(r.size),
            bold: Some(r.bold),
            italic: Some(r.italic),
            underline: Some(r.underline),
            strike: Some(r.strike),
            color: Some(r.color),
            shading: r.shading,
            vert_align: Some(r.vert_align),
            ..Default::default()
        }
    }
    pub fn formatting(&self) -> FormatState {
        let s = self.session.borrow();
        let state = wordcraft_engine::cmd::format::state(&s);
        let para = wordcraft_engine::cmd::para::selection_para(&s);
        let style = s.doc.styles.resolve_char(
            s.doc
                .para_at(&s.sel.focus)
                .and_then(|p| p.props.style.as_deref()),
            &s.caret_props(),
        );
        FormatState {
            font: state["font"].as_str().unwrap_or(&style.font).to_owned(),
            size: state["size"].as_f64().unwrap_or(f64::from(style.size)) as f32,
            bold: state["bold"].as_bool().unwrap_or(false),
            italic: state["italic"].as_bool().unwrap_or(false),
            underline: state["underline"].as_bool().unwrap_or(false),
            strikethrough: state["strike"].as_bool().unwrap_or(false),
            foreground: match style.color {
                TextColor::Auto => 0,
                TextColor::Rgb(c) => rgb_value(c),
            },
            background: style
                .shading
                .or(style.highlight)
                .map(rgb_value)
                .unwrap_or(0xffffff),
            alignment: match para.align.visual(para.bidi) {
                Align::Center => Alignment::Center,
                Align::Right => Alignment::Right,
                Align::Justify => Alignment::Justify,
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
        self.run_command("insert.table", &json!({"rows":rows,"cols":columns}))?;
        Ok(())
    }
    pub fn insert_link(&self, url: &str, label: &str) -> Result<()> {
        ensure!(!self.is_plain(), "formatted links require rich-text mode");
        ensure!(
            !url.is_empty() && !url.chars().any(|c| matches!(c, '\r' | '\n' | '\0' | '"')),
            "enter a valid link without quotes or line breaks"
        );
        ensure!(
            wordcraft_doc::link::is_safe_href(url),
            "Use a web, email, bookmark or relative link."
        );
        validate_text(label)?;
        self.run_command(
            "insert.link",
            &json!({"url":url,"text":if label.is_empty(){url}else{label}}),
        )?;
        Ok(())
    }
    pub fn insert_image(&self, path: &Path) -> Result<()> {
        ensure!(!self.is_plain(), "pictures require rich-text mode");
        self.run_command(
            "insert.picture",
            &json!({"path":path.to_str().context("the image path is not valid Unicode")?}),
        )?;
        Ok(())
    }
    pub fn preview(&self, rtf: &[u8]) -> Result<Self> {
        let preview = Self::new()?;
        preview.set_rtf(rtf)?;
        *preview.display.borrow_mut() = self.display.borrow().clone();
        preview.set_read_only(true);
        preview.mark_saved();
        Ok(preview)
    }
    pub fn page_settings(&self) -> PageSettings {
        let session = self.session.borrow();
        let section = &session.doc.last_section;
        PageSettings {
            width_twips: (section.page_w * 20.0).round() as i32,
            height_twips: (section.page_h * 20.0).round() as i32,
            margin_left: (section.margin_left * 20.0).round() as i32,
            margin_right: (section.margin_right * 20.0).round() as i32,
            margin_top: (section.margin_top * 20.0).round() as i32,
            margin_bottom: (section.margin_bottom * 20.0).round() as i32,
        }
    }
    pub fn set_page_settings(&self, page: &PageSettings) -> Result<()> {
        self.run_command("exeditor.pageSettings", &json!({"width_twips":page.width_twips,"height_twips":page.height_twips,
            "margin_left":page.margin_left,"margin_right":page.margin_right,"margin_top":page.margin_top,"margin_bottom":page.margin_bottom}))?;
        Ok(())
    }
    pub fn export_pdf(&self, path: &Path, page: &PageSettings) -> Result<()> {
        printing::export_pdf(&self.snapshot(), path, page)
    }
    pub fn set_visible(&self, visible: bool) {
        if self.display.borrow().visible != visible {
            self.display.borrow_mut().visible = visible;
            self.invalidate_display();
        }
    }
    pub fn focus(&self) {
        if self.display.borrow().visible {
            self.focus_requested.set(true);
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
            self.view_scroll.set((x.max(0.0), y.max(0.0)));
        }
    }
    pub fn enqueue_shortcut(&self, shortcut: &str) {
        self.shortcuts.borrow_mut().push_back(shortcut.to_owned());
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
        let p = percent.clamp(10, 500);
        if self.display.borrow().zoom != p {
            self.display.borrow_mut().zoom = p;
            self.invalidate_display();
        }
    }
    pub fn set_wrap(&self, wrap: bool) {
        if self.display.borrow().wrap != wrap {
            self.display.borrow_mut().wrap = wrap;
            self.invalidate_display();
        }
    }
    pub fn set_wrap_to_page(&self, value: bool) {
        if self.display.borrow().wrap_to_page != value {
            self.display.borrow_mut().wrap_to_page = value;
            self.invalidate_display();
        }
    }
    pub fn set_margins(&self, left: i32, right: i32) {
        let margins = (left.max(0), right.max(0));
        if self.display.borrow().margins != margins {
            self.display.borrow_mut().margins = margins;
            self.invalidate_display();
        }
    }
    pub fn set_background(&self, color: u32) {
        let color = color & 0xffffff;
        if self.display.borrow().background != color {
            self.display.borrow_mut().background = color;
            self.invalidate_display();
        }
    }
    pub fn set_text_override(&self, color: Option<u32>) -> Result<()> {
        let color = color.map(|c| c & 0xffffff);
        if self.display.borrow().text_override != color {
            self.display.borrow_mut().text_override = color;
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn set_auto_links(&self, enabled: bool) {
        self.display.borrow_mut().auto_links = enabled;
        self.session.borrow_mut().prefs.autocorrect.links = enabled && !self.is_plain();
    }
    pub fn set_autocomplete_enabled(&self, value: bool) {
        if self.display.borrow().autocomplete_enabled != value {
            self.display.borrow_mut().autocomplete_enabled = value;
            self.invalidate_display();
        }
    }
    pub fn set_completion_language(&self, value: Option<String>) {
        let value = value.unwrap_or_default();
        if self.display.borrow().completion_language != value {
            self.display.borrow_mut().completion_language = value;
            self.invalidate_display();
        }
    }
    pub fn set_viewport_width(&self, width: f32) {
        if width.is_finite() && width > 0.0 && (self.viewport_width.get() - width).abs() > 0.25 {
            self.viewport_width.set(width);
            self.invalidate_display();
        }
    }
    pub fn apply_spans(&self, spans: &[crate::syntax::Span]) -> Result<()> {
        ensure!(
            self.is_plain() || spans.is_empty(),
            "source coloring requires plain-text mode"
        );
        let text = self.text()?;
        for span in spans {
            ensure!(
                span.start
                    .checked_add(span.len)
                    .is_some_and(
                        |end| core_positions::utf16_to_byte(&text, span.start).is_some()
                            && core_positions::utf16_to_byte(&text, end).is_some()
                    ),
                "a syntax span is outside the document"
            );
        }
        if self.display.borrow().syntax != spans {
            self.display.borrow_mut().syntax = spans.to_vec();
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn clear_spans(&self) -> Result<()> {
        if !self.display.borrow().syntax.is_empty() {
            self.display.borrow_mut().syntax.clear();
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn set_proofing_marks(&self, ranges: &[Range<usize>]) -> Result<()> {
        self.with_map(|m| {
            for range in ranges {
                m.slice(range.clone())?;
            }
            Ok::<_, anyhow::Error>(())
        })?;
        if self.display.borrow().proofing != ranges {
            self.display.borrow_mut().proofing = ranges.to_vec();
            self.invalidate_display();
        }
        Ok(())
    }
    pub fn adapt_automatic_text(&self) -> Result<()> {
        self.invalidate_display();
        Ok(())
    }
    fn ensure_display(&self) {
        let key = self.revision();
        if self
            .rendered
            .borrow()
            .as_ref()
            .is_some_and(|d| d.key == key)
        {
            return;
        }
        let display = self.display.borrow().clone();
        let mut document = self.snapshot();
        if let Some(color) = display.text_override {
            let styles = document.styles.clone();
            for story in std::iter::once(StoryRef::Body)
                .chain(document.parts.keys().copied().map(StoryRef::Part))
                .collect::<Vec<_>>()
            {
                for path in document.para_paths(story) {
                    if let Ok(p) = document.para_mut(story, &path) {
                        let style = p.props.style.clone();
                        for run in &mut p.runs {
                            if matches!(
                                styles.resolve_char(style.as_deref(), &run.props).color,
                                TextColor::Auto | TextColor::Rgb(Rgb(0, 0, 0))
                            ) {
                                run.props.color = Some(TextColor::Rgb(rgb(color)));
                            }
                        }
                        if matches!(
                            styles.resolve_char(style.as_deref(), &p.mark).color,
                            TextColor::Auto | TextColor::Rgb(Rgb(0, 0, 0))
                        ) {
                            p.mark.color = Some(TextColor::Rgb(rgb(color)));
                        }
                        p.touch();
                    }
                }
            }
        }
        if !display.syntax.is_empty() {
            let map = core_positions::TextMap::new(&document);
            for span in &display.syntax {
                if let (Ok(a), Ok(b)) = (
                    map.position(span.start),
                    map.position(span.start + span.len),
                ) {
                    let _ = document.format_range(&a, &b, &|p| {
                        p.color = Some(TextColor::Rgb(rgb(span.color)));
                        p.bold = Some(span.bold);
                        p.italic = Some(span.italic);
                    });
                }
            }
        }
        let mode = if display.wrap_to_page && !display.plain && display.wrap {
            ViewMode::Print
        } else {
            ViewMode::Web
        };
        let width = if display.wrap {
            self.viewport_width.get()
        } else {
            let longest = self.with_map(|m| {
                m.text
                    .lines()
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(0)
            });
            (longest as f32 * self.formatting().size.max(12.0) * 2.0 + 100.0)
                .max(self.viewport_width.get())
        };
        let options = LayoutOptions {
            view: mode,
            web_width: width,
            proofing: false,
            ..LayoutOptions::default()
        };
        let layout = Arc::new(wordcraft_layout::layout(
            &document,
            &mut self.layout_cache.borrow_mut(),
            &options,
        ));
        *self.rendered.borrow_mut() = Some(DisplayDocument {
            key,
            document,
            layout,
        });
    }
    pub fn display_snapshot(&self) -> Document {
        self.ensure_display();
        self.rendered
            .borrow()
            .as_ref()
            .expect("display initialized")
            .document
            .clone()
    }
    pub fn layout(&self) -> Arc<DocLayout> {
        self.ensure_display();
        self.rendered
            .borrow()
            .as_ref()
            .expect("display initialized")
            .layout
            .clone()
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
fn rgb(color: u32) -> Rgb {
    Rgb((color >> 16) as u8, (color >> 8) as u8, color as u8)
}
fn rgb_value(color: Rgb) -> u32 {
    (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2)
}

#[cfg(test)]
mod core_tests;
