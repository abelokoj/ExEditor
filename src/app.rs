//! Document windows and commands shared by menus, toolbar, and native shortcuts.
use crate::{
    color::{ColorTarget, Rgba},
    color_picker,
    dialogs::{self, Commands, Field, FormKind, MenuEntry},
    document::{
        self, DocumentContent, DocumentFormat, FileFingerprint, LineEnding, RecoveryDocument,
        TextEncoding,
    },
    native::{Alignment, Case, EditCommand, FormatState, ListKind, NativeEditor, PageSettings},
    preferences::{Appearance, Preferences},
    services::{self, Speech},
    session::{self, BufferLease, RestoredBuffer},
    ui::{self, Palette},
};
use anyhow::{Context as _, Result, bail, ensure};
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::{prelude::FluentBuilder as _, *};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::{
    cell::RefCell,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub enum MenuKind {
    App,
    File,
    Recent,
    Edit,
    Find,
    Spelling,
    Substitutions,
    Transformations,
    Format,
    Font,
    Alignment,
    Lists,
    View,
    Window,
    Help,
    Fonts,
    Sizes,
    Weights,
    Spacing,
    Versions,
}
#[derive(Clone, Debug)]
pub enum Command {
    New,
    NewTab,
    SwitchTab(usize),
    NextTab(bool),
    ToggleTabs,
    CloseWindow,
    Open,
    OpenBundle,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Duplicate,
    Rename,
    Move,
    Revert,
    Restore(PathBuf),
    Close,
    Quit,
    Print,
    Pdf,
    PageSetup,
    Properties,
    Find,
    Replace,
    FindNext,
    FindPrevious,
    UseSelection,
    GoToSelection,
    GoToLine,
    ToggleFormat,
    ToggleRuler,
    ToggleWrap,
    ToggleReadOnly,
    Zoom(i16),
    Preferences,
    ClearRecent,
    Font,
    Foreground,
    CustomSpacing,
    Background,
    InsertColorCode,
    ApplyColor(ColorTarget, Rgba, String),
    Table,
    Link,
    Image,
    CheckSpelling,
    Correct(std::ops::Range<usize>, String),
    Complete,
    Speak,
    StopSpeaking,
    Toggle(&'static str),
    Edit(EditCommand),
    Menu(MenuKind),
    Submit(FormKind, Vec<String>),
    About,
    Help,
}

pub fn show_error(error: &anyhow::Error) {
    MessageDialog::new()
        .set_title("ExEditor")
        .set_description(format!("{error:#}"))
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show();
}
fn confirm(title: &str, message: &str) -> bool {
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_buttons(MessageButtons::OkCancel)
        .show()
        == MessageDialogResult::Ok
}

pub fn open_document(path: Option<PathBuf>, cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(760.), px(620.)), cx)),
        window_min_size: Some(size(px(590.), px(300.))),
        titlebar: None,
        app_owns_titlebar_drag: true,
        app_id: Some("ExEditor".into()),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| EditorWindow::new(path, window, cx))
    });
    if let Err(error) = result {
        show_error(&error);
    }
}

/// Restores protected buffers automatically, even when startup also opens files.
pub fn restore_session(cx: &mut App) -> bool {
    let restored = match session::restore_buffers() {
        Ok(restored) => restored,
        Err(error) => {
            show_error(&error);
            return false;
        }
    };
    if !restored.warnings.is_empty() {
        show_error(&anyhow::anyhow!(restored.warnings.join("\n\n")));
    }
    if restored.buffers.is_empty() {
        return false;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(760.), px(620.)), cx)),
        window_min_size: Some(size(px(590.), px(300.))),
        titlebar: None,
        app_owns_titlebar_drag: true,
        app_id: Some("ExEditor".into()),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let mut view = EditorWindow::new(None, window, cx);
            let parent = match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw())
            {
                Ok(RawWindowHandle::Win32(handle)) => Some(handle.hwnd.get()),
                _ => None,
            };
            let mut count = 0;
            for buffer in restored.buffers {
                let result = parent
                    .context("The recovery window could not create its text editor")
                    .and_then(|parent| DocumentTab::restore(&view.prefs, parent, buffer));
                match result {
                    Ok(tab) => {
                        if count == 0 {
                            view.doc = tab;
                        } else {
                            if let Some(editor) = &tab.native {
                                editor.set_visible(false);
                            }
                            view.tabs.push(Some(tab));
                        }
                        count += 1;
                    }
                    Err(error) => show_error(
                        &error.context("The unsaved snapshot is still protected on disk"),
                    ),
                }
            }
            view.show_tabs = count > 1;
            if let Some(editor) = &view.doc.native {
                editor.set_visible(true);
                editor.focus();
            }
            view
        })
    }) {
        Ok(_) => true,
        Err(error) => {
            show_error(&error);
            false
        }
    }
}

struct DocumentTab {
    native: Option<Rc<NativeEditor>>,
    path: Option<PathBuf>,
    format: DocumentFormat,
    encoding: TextEncoding,
    line_ending: LineEnding,
    fingerprint: Option<FileFingerprint>,
    rich: bool,
    dirty: bool,
    read_only: bool,
    ruler: bool,
    wrap_page: bool,
    zoom: u16,
    formatting: FormatState,
    page: PageSettings,
    recovery_id: String,
    buffer_lease: Option<BufferLease>,
    buffered: bool,
    retain_buffer: bool,
    buffer_error_reported: bool,
    restored_buffer: bool,
    last_recovery: Instant,
    last_text: String,
    find_text: String,
    replace_text: String,
    find_case: bool,
    find_whole: bool,
    last_external_check: Instant,
}
struct EditorWindow {
    doc: DocumentTab,
    tabs: Vec<Option<DocumentTab>>,
    active_tab: usize,
    show_tabs: bool,
    queue: Commands,
    prefs: Preferences,
    speech: Speech,
    _poll: Task<()>,
}

impl EditorWindow {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Preferences::load().unwrap_or_else(|error| {
            show_error(&error);
            Preferences::default()
        });
        let hwnd = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
            _ => 0,
        };
        let doc = DocumentTab::new(&prefs, hwnd);
        let mut this = Self {
            doc,
            tabs: vec![None],
            active_tab: 0,
            show_tabs: false,
            queue: Rc::new(RefCell::new(VecDeque::new())),
            prefs,
            speech: Speech::default(),
            _poll: Task::ready(()),
        };
        if let Some(path) = path
            && let Err(error) = this.read_file(&path)
        {
            show_error(&error);
        }
        if !cx.has_global::<OpenEditors>() {
            cx.set_global(OpenEditors::default());
        }
        let registration = (Window::window_handle(window), cx.entity().downgrade());
        cx.global_mut::<OpenEditors>().0.push(registration);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.confirm_close(window, cx))
                .unwrap_or(true)
        });
        let handle = Window::window_handle(window);
        this._poll = cx.spawn(async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(100)).await;
                if handle
                    .update(cx, |_, window, cx| {
                        this.update(cx, |this, cx| this.tick(window, cx))
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        this
    }
    fn editor(&self) -> Result<Rc<NativeEditor>> {
        self.doc
            .native
            .clone()
            .context("The native text editor could not be created")
    }
    fn colors(&self, window: &Window) -> Palette {
        Palette::new(match self.prefs.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        })
    }
    fn name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }
    fn content(&self) -> Result<DocumentContent> {
        let editor = self.editor()?;
        if self.doc.rich {
            Ok(DocumentContent::Rich(editor.rtf()?))
        } else {
            Ok(DocumentContent::Plain(editor.text()?))
        }
    }
    fn load_content(&self, content: &DocumentContent) -> Result<()> {
        let editor = self.editor()?;
        match content {
            DocumentContent::Plain(text) => editor.set_text(text)?,
            DocumentContent::Rich(bytes) => editor.set_rtf(bytes)?,
        }
        editor.mark_modified();
        Ok(())
    }
    fn read_file(&mut self, path: &Path) -> Result<()> {
        let format = DocumentFormat::from_path(path)?;
        ensure!(
            document::supports_format(format),
            "This document format is not supported. Save it as RTF or DOCX in the originating application."
        );
        let loaded = document::load(path).or_else(|error| {
            if format.is_plain() && self.prefs.default_encoding != TextEncoding::Utf8 {
                document::load_with_encoding(path, self.prefs.default_encoding)
            } else {
                Err(error)
            }
        })?;
        self.editor()?
            .set_plain_text_mode(loaded.format.is_plain())?;
        self.load_content(&loaded.content)?;
        self.doc.rich = !loaded.format.is_plain();
        self.doc.path = Some(loaded.path.clone());
        self.doc.format = loaded.format;
        self.doc.encoding = loaded.encoding;
        self.doc.line_ending = loaded.line_ending;
        self.doc.fingerprint = Some(loaded.fingerprint);
        self.doc.dirty = false;
        self.editor()?.mark_saved();
        document::remove_recovery(&self.doc.recovery_id)?;
        self.doc.buffered = false;
        self.doc.restored_buffer = false;
        self.prefs.add_recent(loaded.path);
        self.prefs.save()?;
        self.doc.last_text = self.editor()?.text()?;
        Ok(())
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(command) = { self.queue.borrow_mut().pop_front() } {
            self.command(command, window, cx);
        }
        let Some(editor) = self.doc.native.clone() else {
            return;
        };
        while let Some(shortcut) = editor.take_shortcut() {
            let command = match shortcut.as_str() {
                "new" => Command::New,
                "open" => Command::Open,
                "save" => Command::Save,
                "save-as" => Command::SaveAs,
                "find" => Command::Find,
                "replace" => Command::Replace,
                "find-next" => Command::FindNext,
                "print" => Command::Print,
                "close" => Command::Close,
                "toggle-format" => Command::ToggleFormat,
                "zoom-in" => Command::Zoom(10),
                "zoom-out" => Command::Zoom(-10),
                "actual-size" => Command::Zoom(0),
                "preferences" => Command::Preferences,
                "quit" => Command::Quit,
                "font" => Command::Font,
                "find-previous" => Command::FindPrevious,
                "use-selection" => Command::UseSelection,
                "go-to-line" => Command::GoToLine,
                "page-setup" => Command::PageSetup,
                "new-tab" => Command::NewTab,
                "next-tab" => Command::NextTab(false),
                "previous-tab" => Command::NextTab(true),
                _ => continue,
            };
            self.command(command, window, cx);
        }
        let Some(editor) = self.doc.native.clone() else {
            return;
        };
        self.doc.dirty = editor.is_modified() || self.doc.dirty;
        self.doc.formatting = editor.formatting();
        if let Ok(text) = editor.text()
            && text != self.doc.last_text
            && window.is_window_active()
            && !self.doc.read_only
        {
            let range = editor.selection();
            if range.start == range.end
                && text.chars().count() == self.doc.last_text.chars().count() + 1
                && let Some(change) = services::substitution(
                    &text,
                    range.end,
                    self.prefs.smart_quotes,
                    self.prefs.smart_dashes,
                )
            {
                let _ = editor
                    .select_range(change.range)
                    .and_then(|()| editor.replace_current(&change.replacement));
            }
            self.doc.last_text = editor.text().unwrap_or(text);
        }
        let needs_buffer = editor.is_modified()
            || (!self.doc.buffered && (self.doc.dirty || self.doc.path.is_none()))
            || (self.doc.dirty
                && self.doc.last_recovery.elapsed()
                    >= Duration::from_secs(self.prefs.autosave_seconds.max(5)));
        if needs_buffer
            && (!self.doc.buffer_error_reported
                || self.doc.last_recovery.elapsed() >= Duration::from_secs(5))
            && let Err(error) = self.doc.persist_buffer()
        {
            if !self.doc.buffer_error_reported {
                show_error(&error);
            }
            self.doc.buffer_error_reported = true;
            self.doc.last_recovery = Instant::now();
        }
        if self.doc.last_external_check.elapsed() > Duration::from_secs(3) {
            if let (Some(path), Some(old)) = (&self.doc.path, &self.doc.fingerprint)
                && let Ok(new) = document::fingerprint(path)
                && &new != old
            {
                let path = path.clone();
                if !self.doc.dirty
                    && confirm(
                        "Document changed on disk",
                        "Reload the changes made by another application?",
                    )
                    && let Err(error) = self.read_file(&path)
                {
                    show_error(&error);
                }
            }
            self.doc.last_external_check = Instant::now();
        }
        window.set_window_title(&format!(
            "{}{} — ExEditor",
            self.name(),
            if self.doc.dirty { " — Edited" } else { "" }
        ));
        cx.notify();
    }
    fn save(&mut self, as_new: bool) -> Result<bool> {
        let path = if as_new || self.doc.path.is_none() {
            let chosen = FileDialog::new()
                .set_title("Save")
                .set_file_name(self.name())
                .add_filter("Rich Text Document", &["rtf"])
                .add_filter("Plain Text Document", &["txt"])
                .add_filter("Rich Text with Attachments", &["rtfd"])
                .add_filter("Web Page", &["html"])
                .add_filter("Word Document", &["docx"])
                .add_filter("OpenDocument Text", &["odt"])
                .save_file();
            let Some(mut chosen) = chosen else {
                return Ok(false);
            };
            if chosen.extension().is_none() {
                chosen.set_extension(self.doc.format.extension());
            }
            chosen
        } else {
            self.doc
                .path
                .clone()
                .context("A saved document must have a path")?
        };
        let changed_on_disk = match (&self.doc.fingerprint, document::fingerprint(&path)) {
            (Some(old), Ok(now)) => old != &now,
            _ => false,
        };
        if !as_new
            && path.try_exists()?
            && (self.doc.restored_buffer || changed_on_disk)
            && !confirm(
                "Replace saved document?",
                if self.doc.restored_buffer {
                    "This recovered buffer may differ from the file currently on disk. Replace the saved file with your recovered changes? Choose Save As to keep both."
                } else {
                    "This file has changed outside ExEditor. Replace it with your current document?"
                },
            )
        {
            return Ok(false);
        }
        let format = DocumentFormat::from_path(&path)?;
        if format.is_plain()
            && self.doc.rich
            && !confirm(
                "Save as plain text?",
                "Plain text removes all formatting, tables, and images. Save a plain-text copy?",
            )
        {
            return Ok(false);
        }
        let content = if format.is_plain() {
            DocumentContent::Plain(self.editor()?.text()?)
        } else {
            DocumentContent::Rich(self.editor()?.rtf()?)
        };
        let fingerprint = document::save(
            &path,
            format,
            &content,
            self.doc.encoding,
            self.doc.line_ending,
        )?;
        self.doc.path = Some(path.clone());
        self.doc.format = format;
        self.doc.fingerprint = Some(fingerprint);
        self.doc.rich = !format.is_plain();
        self.editor()?.set_plain_text_mode(format.is_plain())?;
        self.doc.dirty = false;
        self.editor()?.mark_saved();
        document::remove_recovery(&self.doc.recovery_id)?;
        self.doc.buffered = false;
        self.doc.restored_buffer = false;
        self.prefs.add_recent(path);
        self.prefs.save()?;
        Ok(true)
    }
    fn confirm_current(&mut self) -> Result<bool> {
        self.doc.dirty |= self.editor()?.is_modified();
        if !self.doc.dirty {
            document::remove_recovery(&self.doc.recovery_id)?;
            self.doc.retain_buffer = false;
            return Ok(true);
        }
        let result = MessageDialog::new()
            .set_title(format!("Save changes to ‘{}’?", self.name()))
            .set_description("Closing this tab discards its unsaved buffer unless you save it. Closing the window keeps all buffers for the next startup.")
            .set_buttons(MessageButtons::YesNoCancelCustom(
                "Save".into(), "Discard".into(), "Cancel".into(),
            )).show();
        let saved = match result {
            MessageDialogResult::Yes => self.save(false)?,
            MessageDialogResult::Custom(ref label) if label == "Save" => self.save(false)?,
            MessageDialogResult::No => {
                document::remove_recovery(&self.doc.recovery_id)?;
                true
            }
            MessageDialogResult::Custom(ref label) if label == "Discard" => {
                document::remove_recovery(&self.doc.recovery_id)?;
                true
            }
            _ => false,
        };
        if saved {
            self.doc.retain_buffer = false;
        }
        Ok(saved)
    }
    fn confirm_close(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        let result = self.doc.persist_buffer().and_then(|()| {
            for tab in self.tabs.iter_mut().flatten() {
                tab.persist_buffer()?;
            }
            Ok(())
        });
        match result {
            Ok(()) => true,
            Err(error) => {
                show_error(&error.context("ExEditor must keep this window open because its unsaved buffer could not be protected. Save the affected document or free disk space, then try closing again."));
                false
            }
        }
    }
    fn switch_tab(&mut self, index: usize) -> Result<()> {
        ensure!(
            index < self.tabs.len(),
            "That document tab is no longer open"
        );
        if index == self.active_tab {
            return Ok(());
        }
        self.doc.persist_buffer()?;
        let next = self.tabs[index]
            .take()
            .context("The document tab is unavailable")?;
        if let Some(editor) = &self.doc.native {
            editor.set_visible(false);
        }
        let previous = std::mem::replace(&mut self.doc, next);
        self.tabs[self.active_tab] = Some(previous);
        self.active_tab = index;
        if let Some(editor) = &self.doc.native {
            editor.set_visible(true);
            editor.focus();
        }
        Ok(())
    }
    fn new_tab(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> Result<()> {
        let hwnd = match HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("Native window handle: {error:?}"))?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => handle.hwnd.get(),
            _ => bail!("A Windows document window is required"),
        };
        let doc = DocumentTab::new(&self.prefs, hwnd);
        if let Some(editor) = &doc.native {
            editor.set_visible(false);
        }
        self.tabs.push(Some(doc));
        self.show_tabs = true;
        self.switch_tab(self.tabs.len() - 1)
    }
    fn close_tab(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> Result<()> {
        if !self.confirm_current()? {
            return Ok(());
        }
        if self.tabs.len() == 1 {
            window.remove_window();
            return Ok(());
        }
        let next = if self.active_tab == 0 {
            1
        } else {
            self.active_tab - 1
        };
        let doc = self.tabs[next]
            .take()
            .context("The next document tab is unavailable")?;
        if let Some(editor) = &self.doc.native {
            editor.set_visible(false);
        }
        self.doc = doc;
        let old = self.active_tab;
        self.tabs.remove(old);
        self.active_tab = if next > old { next - 1 } else { next };
        if let Some(editor) = &self.doc.native {
            editor.set_visible(true);
            editor.focus();
        }
        Ok(())
    }
    fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.try_command(command, window, cx) {
            show_error(&error);
        }
        cx.notify();
    }
    fn try_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        match command {
            Command::New => open_document(None, cx),
            Command::NewTab => self.new_tab(window, cx)?,
            Command::SwitchTab(index) => self.switch_tab(index)?,
            Command::NextTab(backwards) => {
                let count = self.tabs.len();
                let index = if backwards {
                    (self.active_tab + count - 1) % count
                } else {
                    (self.active_tab + 1) % count
                };
                self.switch_tab(index)?;
            }
            Command::ToggleTabs => self.show_tabs = !self.show_tabs,
            Command::CloseWindow => {
                if self.confirm_close(window, cx) {
                    window.remove_window();
                }
            }
            Command::Open => {
                if let Some(paths) = FileDialog::new()
                    .set_title("Open")
                    .add_filter(
                        "Documents",
                        &["txt", "rtf", "rtfd", "html", "htm", "docx", "odt"],
                    )
                    .pick_files()
                {
                    for path in paths {
                        open_document(Some(path), cx);
                    }
                }
            }
            Command::OpenBundle => {
                if let Some(path) = FileDialog::new()
                    .set_title("Open Rich Text with Attachments")
                    .pick_folder()
                {
                    open_document(Some(path), cx);
                }
            }
            Command::OpenPath(path) => open_document(Some(path), cx),
            Command::Save => {
                self.save(false)?;
            }
            Command::SaveAs => {
                self.save(true)?;
            }
            Command::Duplicate => {
                let content = self.content()?;
                let rich = self.doc.rich;
                let format = self.doc.format;
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::centered(size(px(760.), px(620.)), cx)),
                        titlebar: None,
                        ..Default::default()
                    },
                    cx,
                    move |window, cx| {
                        cx.new(|cx| {
                            let mut view = Self::new(None, window, cx);
                            view.doc.rich = rich;
                            view.doc.format = format;
                            if let Some(editor) = &view.doc.native {
                                let _ = editor.set_plain_text_mode(!rich);
                            }
                            if let Err(error) = view.load_content(&content) {
                                show_error(&error);
                            }
                            view.doc.dirty = true;
                            view
                        })
                    },
                )?;
            }
            Command::Rename | Command::Move => {
                if !self.save(false)? {
                    return Ok(());
                }
                let old = self
                    .doc
                    .path
                    .clone()
                    .context("The document must be saved before it can be moved")?;
                if let Some(destination) = FileDialog::new()
                    .set_title("Move or Rename Document")
                    .set_file_name(self.name())
                    .save_file()
                {
                    if destination == old {
                        return Ok(());
                    }
                    ensure!(
                        !destination.exists(),
                        "A file already exists at that location. Choose another name."
                    );
                    std::fs::rename(&old, &destination).context(
                        "Moving the document; the original remains in place if this fails",
                    )?;
                    self.doc.fingerprint = Some(document::fingerprint(&destination)?);
                    self.doc.path = Some(destination.clone());
                    self.prefs.add_recent(destination);
                    self.prefs.save()?;
                }
            }
            Command::Revert => {
                if let Some(path) = self.doc.path.clone()
                    && confirm(
                        "Revert document?",
                        "Replace your current changes with the version saved on disk?",
                    )
                {
                    self.read_file(&path)?;
                }
            }
            Command::Restore(path) => {
                if confirm(
                    "Restore this version?",
                    "Your current document will be replaced. You can save the restored version when ready.",
                ) {
                    let loaded = document::load(&path)?;
                    self.load_content(&loaded.content)?;
                    self.doc.dirty = true;
                }
            }
            Command::Close => self.close_tab(window, cx)?,
            Command::Quit => {
                if !self.confirm_close(window, cx) {
                    return Ok(());
                }
                let editors = cx.global::<OpenEditors>().0.clone();
                let own = cx.entity().entity_id();
                for (handle, editor) in editors {
                    if editor.entity_id() == own {
                        continue;
                    }
                    let can_close = handle
                        .update(cx, |_, window, cx| {
                            editor.update(cx, |view, cx| view.confirm_close(window, cx))
                        })
                        .ok()
                        .and_then(Result::ok)
                        .unwrap_or(true);
                    if !can_close {
                        return Ok(());
                    }
                }
                cx.quit();
            }
            Command::Print => self.editor()?.print(&self.doc.page)?,
            Command::Pdf => {
                if let Some(path) = FileDialog::new()
                    .set_title("Export as PDF")
                    .set_file_name(format!(
                        "{}.pdf",
                        self.name()
                            .trim_end_matches(self.doc.format.extension())
                            .trim_end_matches('.')
                    ))
                    .add_filter("PDF Document", &["pdf"])
                    .save_file()
                {
                    self.editor()?.export_pdf(&path, &self.doc.page)?;
                }
            }
            Command::PageSetup => self.show_form(
                FormKind::PageSetup,
                "Page Setup",
                vec![
                    Field::text("Paper width (in)", self.doc.page.width_twips as f32 / 1440.),
                    Field::text(
                        "Paper height (in)",
                        self.doc.page.height_twips as f32 / 1440.,
                    ),
                    Field::text("Left margin (in)", self.doc.page.margin_left as f32 / 1440.),
                    Field::text(
                        "Right margin (in)",
                        self.doc.page.margin_right as f32 / 1440.,
                    ),
                    Field::text("Top margin (in)", self.doc.page.margin_top as f32 / 1440.),
                    Field::text(
                        "Bottom margin (in)",
                        self.doc.page.margin_bottom as f32 / 1440.,
                    ),
                ],
                window,
                cx,
            ),
            Command::Properties => {
                let text = self.editor()?.text()?;
                MessageDialog::new()
                    .set_title("Document Properties")
                    .set_description(format!(
                        "Name: {}\nKind: {:?}\nEncoding: {}\nWords: {}\nCharacters: {}\nLines: {}",
                        self.name(),
                        self.doc.format,
                        self.doc.encoding.label(),
                        text.split_whitespace().count(),
                        text.chars().count(),
                        text.lines().count()
                    ))
                    .show();
            }
            Command::Find => self.show_form(
                FormKind::Find,
                "Find",
                vec![
                    Field::text("Find", &self.doc.find_text),
                    Field::toggle("Match case", self.doc.find_case),
                    Field::toggle("Whole words", self.doc.find_whole),
                ],
                window,
                cx,
            ),
            Command::Replace => self.show_form(
                FormKind::Replace,
                "Find and Replace",
                vec![
                    Field::text("Find", &self.doc.find_text),
                    Field::text("Replace with", &self.doc.replace_text),
                    Field::toggle("Match case", self.doc.find_case),
                    Field::toggle("Whole words", self.doc.find_whole),
                ],
                window,
                cx,
            ),
            Command::FindNext | Command::FindPrevious => {
                if self.doc.find_text.is_empty() {
                    self.command(Command::Find, window, cx);
                } else {
                    let backwards = matches!(command, Command::FindPrevious);
                    if !self.editor()?.find(
                        &self.doc.find_text,
                        self.doc.find_case,
                        self.doc.find_whole,
                        backwards,
                    )? {
                        MessageDialog::new()
                            .set_title("Find")
                            .set_description("No matches found.")
                            .show();
                    }
                }
            }
            Command::UseSelection => {
                self.doc.find_text = self.editor()?.selected_text()?;
            }
            Command::GoToSelection => self.editor()?.focus(),
            Command::GoToLine => self.show_form(
                FormKind::GoToLine,
                "Go to Line",
                vec![Field::text("Line number", 1)],
                window,
                cx,
            ),
            Command::ToggleFormat => {
                if self.doc.rich
                    && !confirm(
                        "Make Plain Text?",
                        "Converting to plain text removes text styles, tables, images, and attachments. Continue?",
                    )
                {
                    return Ok(());
                }
                self.doc.rich = !self.doc.rich;
                self.editor()?.set_plain_text_mode(!self.doc.rich)?;
                self.doc.format = if self.doc.rich {
                    DocumentFormat::Rtf
                } else {
                    DocumentFormat::PlainText
                };
                self.doc.path = None;
                self.doc.fingerprint = None;
                self.doc.dirty = true;
                self.editor()?.execute(EditCommand::Font(if self.doc.rich {
                    self.prefs.rich_font.clone()
                } else {
                    self.prefs.plain_font.clone()
                }))?;
                self.editor()?
                    .execute(EditCommand::FontSize(if self.doc.rich {
                        self.prefs.rich_size
                    } else {
                        self.prefs.plain_size
                    }))?;
            }
            Command::ToggleRuler => {
                self.doc.ruler = !self.doc.ruler;
            }
            Command::ToggleWrap => {
                self.doc.wrap_page = !self.doc.wrap_page;
            }
            Command::ToggleReadOnly => {
                self.doc.read_only = !self.doc.read_only;
                self.editor()?.set_read_only(self.doc.read_only);
            }
            Command::Zoom(delta) => {
                self.doc.zoom = if delta == 0 {
                    100
                } else {
                    (i32::from(self.doc.zoom) + i32::from(delta)).clamp(25, 400) as u16
                };
                self.editor()?.set_zoom(self.doc.zoom);
            }
            Command::Preferences => crate::settings::open(
                self.prefs.clone(),
                self.queue.clone(),
                self.colors(window),
                cx,
            ),
            Command::ClearRecent => {
                self.prefs.clear_recent();
                self.prefs.save()?;
            }
            Command::Font => self.show_form(
                FormKind::Font,
                "Fonts",
                vec![
                    Field::text("Family", &self.doc.formatting.font),
                    Field::text("Size", self.doc.formatting.size),
                    Field::toggle("Bold", self.doc.formatting.bold),
                    Field::toggle("Italic", self.doc.formatting.italic),
                    Field::toggle("Underline", self.doc.formatting.underline),
                ],
                window,
                cx,
            ),
            Command::CustomSpacing => self.show_form(
                FormKind::Spacing,
                "Line Spacing",
                vec![Field::text("Line spacing", 1.0)],
                window,
                cx,
            ),
            Command::Foreground => color_picker::open(
                Rgba::from_rgb(self.editor()?.formatting().foreground),
                ColorTarget::Foreground,
                self.queue.clone(),
                cx,
            ),
            Command::Background => color_picker::open(
                Rgba::from_rgb(self.editor()?.formatting().background),
                ColorTarget::Background,
                self.queue.clone(),
                cx,
            ),
            Command::InsertColorCode => {
                let selected = self.editor()?.selected_text()?;
                color_picker::open_with_code(
                    Rgba::from_rgb(self.editor()?.formatting().foreground),
                    ColorTarget::InsertCode,
                    self.queue.clone(),
                    Some(selected),
                    cx,
                );
            }
            Command::ApplyColor(target, color, code) => {
                let editor = self.editor()?;
                match target {
                    ColorTarget::Foreground => {
                        ensure!(
                            self.doc.rich,
                            "Text colors require a rich-text document. Use Format > Make Rich Text, or Insert Color Code for plain text."
                        );
                        editor.execute(EditCommand::Foreground(
                            color.composite(editor.formatting().background),
                        ))?;
                    }
                    ColorTarget::Background => {
                        ensure!(
                            self.doc.rich,
                            "Text background colors require a rich-text document."
                        );
                        editor.execute(EditCommand::Background(color.composite(0xffffff)))?;
                    }
                    ColorTarget::InsertCode => editor.replace_current(&code)?,
                }
                self.doc.persist_buffer()?;
            }
            Command::Table => self.show_form(
                FormKind::Table,
                "Table",
                vec![Field::text("Rows", 3), Field::text("Columns", 3)],
                window,
                cx,
            ),
            Command::Link => self.show_form(
                FormKind::Link,
                "Link",
                vec![
                    Field::text("Address", "https://"),
                    Field::text("Text", self.editor()?.selected_text()?),
                ],
                window,
                cx,
            ),
            Command::Image => {
                if let Some(path) = FileDialog::new()
                    .set_title("Insert Image")
                    .add_filter("Images", &["png", "jpg", "jpeg", "gif", "bmp", "tiff"])
                    .pick_file()
                {
                    self.editor()?.insert_image(&path)?;
                }
            }
            Command::CheckSpelling => {
                let text = self.editor()?.text()?;
                let issues = services::check_spelling(&text)?;
                let caret = self.editor()?.selection().end;
                if let Some(issue) = issues
                    .iter()
                    .find(|i| i.range.start >= caret)
                    .or(issues.first())
                {
                    self.editor()?.select_range(issue.range.clone())?;
                    let entries = issue
                        .suggestions
                        .iter()
                        .map(|s| {
                            MenuEntry::item(s, "", Command::Correct(issue.range.clone(), s.clone()))
                        })
                        .collect();
                    self.show_menu(entries, window, cx);
                } else {
                    MessageDialog::new()
                        .set_title("Spelling")
                        .set_description("No spelling errors found.")
                        .show();
                }
            }
            Command::Correct(range, text) => {
                self.editor()?.select_range(range)?;
                self.editor()?.replace_current(&text)?;
            }
            Command::Complete => {
                let editor = self.editor()?;
                let text = editor.text()?;
                let caret = editor.selection().end;
                let before =
                    String::from_utf16(&text.encode_utf16().take(caret).collect::<Vec<_>>())?;
                let prefix = before
                    .rsplit(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap_or_default();
                let range = caret - prefix.encode_utf16().count()..caret;
                let entries = services::completions(&text, prefix)
                    .into_iter()
                    .map(|s| MenuEntry::item(&s, "", Command::Correct(range.clone(), s.clone())))
                    .collect();
                self.show_menu(entries, window, cx);
            }
            Command::Speak => {
                let selected = self.editor()?.selected_text()?;
                let text = if selected.is_empty() {
                    self.editor()?.text()?
                } else {
                    selected
                };
                self.speech.start(&text)?;
            }
            Command::StopSpeaking => {
                self.speech.stop()?;
            }
            Command::Toggle(key) => {
                match key {
                    "quotes" => self.prefs.smart_quotes = !self.prefs.smart_quotes,
                    "dashes" => self.prefs.smart_dashes = !self.prefs.smart_dashes,
                    "links" => {
                        self.prefs.automatic_links = !self.prefs.automatic_links;
                        self.editor()?.set_auto_links(self.prefs.automatic_links);
                    }
                    "spelling" => {
                        self.prefs.spellcheck = !self.prefs.spellcheck;
                        self.editor()?.set_spellcheck(self.prefs.spellcheck);
                    }
                    _ => {}
                }
                self.prefs.save()?;
            }
            Command::Edit(edit) => {
                if self.doc.read_only && !matches!(edit, EditCommand::Copy | EditCommand::SelectAll)
                {
                    bail!("This document is locked. Unlock it in the Format menu to make changes.");
                }
                self.editor()?.execute(edit)?;
            }
            Command::Menu(kind) => {
                let entries = self.menu_entries(kind)?;
                self.show_menu(entries, window, cx);
            }
            Command::Submit(kind, values) => self.submit(kind, values, window, cx)?,
            Command::About => {
                MessageDialog::new().set_title("About ExEditor").set_description("ExEditor 0.1.0\nA native document editor built with Rust and GPUI Kit.\nhttps://github.com/abelokoj/ExEditor\n\nTextEdit is a trademark of Apple Inc. ExEditor is an independent application.").show();
            }
            Command::Help => {
                let _ = std::process::Command::new("explorer.exe")
                    .arg("https://support.apple.com/guide/textedit/welcome/mac")
                    .spawn();
            }
        }
        Ok(())
    }
    fn show_form(
        &self,
        kind: FormKind,
        title: &str,
        fields: Vec<Field>,
        window: &Window,
        cx: &mut App,
    ) {
        dialogs::form(
            kind,
            title,
            fields,
            self.queue.clone(),
            self.colors(window),
            cx,
        );
    }
    fn show_menu(&self, entries: Vec<MenuEntry>, window: &Window, cx: &mut App) {
        let position = point(
            window.bounds().origin.x + px(12.),
            window.bounds().origin.y + px(61.),
        );
        dialogs::menu(
            entries,
            position,
            self.queue.clone(),
            self.colors(window),
            cx,
        );
    }
    fn submit(
        &mut self,
        kind: FormKind,
        values: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let value = |index: usize| {
            values
                .get(index)
                .map(String::as_str)
                .context("A required field is missing")
        };
        let number = |index: usize| -> Result<f32> {
            let n = value(index)?.parse::<f32>().context("Enter a number")?;
            ensure!(
                n.is_finite() && n > 0. && n <= 1000.,
                "Enter a number greater than zero and at most 1000"
            );
            Ok(n)
        };
        match kind {
            FormKind::Font => {
                let font = value(0)?.trim();
                ensure!(!font.is_empty(), "Choose a font family");
                let size = number(1)?;
                let editor = self.editor()?;
                editor.execute(EditCommand::Font(font.into()))?;
                editor.execute(EditCommand::FontSize(size))?;
                let current = editor.formatting();
                for (actual, want, cmd) in [
                    (current.bold, value(2)? == "true", EditCommand::Bold),
                    (current.italic, value(3)? == "true", EditCommand::Italic),
                    (
                        current.underline,
                        value(4)? == "true",
                        EditCommand::Underline,
                    ),
                ] {
                    if actual != want {
                        editor.execute(cmd)?;
                    }
                }
            }
            FormKind::Table => {
                let rows = value(0)?.parse::<u16>()?;
                let columns = value(1)?.parse::<u16>()?;
                ensure!(
                    (1..=100).contains(&rows) && (1..=30).contains(&columns),
                    "Use 1–100 rows and 1–30 columns"
                );
                self.editor()?.insert_table(rows, columns)?;
            }
            FormKind::Link => {
                let url = value(0)?.trim();
                ensure!(
                    url.starts_with("https://")
                        || url.starts_with("http://")
                        || url.starts_with("mailto:"),
                    "Use an http, https, or mailto address"
                );
                self.editor()?.insert_link(url, value(1)?)?;
            }
            FormKind::Spacing => self
                .editor()?
                .execute(EditCommand::LineSpacing(number(0)?))?,
            FormKind::PageSetup => {
                let mut page = self.doc.page.clone();
                page.width_twips = (number(0)? * 1440.).round() as i32;
                page.height_twips = (number(1)? * 1440.).round() as i32;
                let margin = |i| -> Result<i32> {
                    let n = value(i)?.parse::<f32>()?;
                    ensure!(n.is_finite() && n >= 0., "Margins must be zero or greater");
                    Ok((n * 1440.).round() as i32)
                };
                page.margin_left = margin(2)?;
                page.margin_right = margin(3)?;
                page.margin_top = margin(4)?;
                page.margin_bottom = margin(5)?;
                ensure!(
                    page.margin_left + page.margin_right < page.width_twips
                        && page.margin_top + page.margin_bottom < page.height_twips,
                    "Margins must leave space for the document"
                );
                self.doc.page = page;
            }
            FormKind::Preferences => {
                let mut prefs = self.prefs.clone();
                prefs.rich_text = value(0)? == "true";
                prefs.rich_font = value(1)?.trim().into();
                prefs.rich_size = number(2)?;
                prefs.plain_font = value(3)?.trim().into();
                prefs.plain_size = number(4)?;
                ensure!(
                    !prefs.rich_font.is_empty() && !prefs.plain_font.is_empty(),
                    "Font families cannot be empty"
                );
                prefs.wrap_to_page = value(5)? == "true";
                prefs.show_ruler = value(6)? == "true";
                prefs.spellcheck = value(7)? == "true";
                prefs.smart_quotes = value(8)? == "true";
                prefs.smart_dashes = value(9)? == "true";
                prefs.automatic_links = value(10)? == "true";
                prefs.autosave_seconds = value(11)?.parse()?;
                ensure!(
                    (5..=3600).contains(&prefs.autosave_seconds),
                    "Recovery interval must be 5–3600 seconds"
                );
                prefs.appearance = match value(12)?.trim().to_lowercase().as_str() {
                    "system" => Appearance::System,
                    "light" => Appearance::Light,
                    "dark" => Appearance::Dark,
                    _ => bail!("Appearance must be system, light, or dark"),
                };
                if let Some(encoding) = values.get(13) {
                    prefs.default_encoding = match encoding.as_str() {
                        "Utf8" => TextEncoding::Utf8,
                        "Utf8Bom" => TextEncoding::Utf8Bom,
                        "Utf16Le" => TextEncoding::Utf16Le,
                        "Utf16Be" => TextEncoding::Utf16Be,
                        "Windows1252" => TextEncoding::Windows1252,
                        _ => bail!("Choose a valid default encoding"),
                    };
                }
                prefs.save()?;
                self.prefs = prefs;
                self.editor()?.set_auto_links(self.prefs.automatic_links);
                self.editor()?.set_spellcheck(self.prefs.spellcheck);
            }
            FormKind::Find => {
                self.doc.find_text = value(0)?.into();
                self.doc.find_case = value(1)? == "true";
                self.doc.find_whole = value(2)? == "true";
                ensure!(!self.doc.find_text.is_empty(), "Enter the text to find");
                self.command(Command::FindNext, window, cx);
            }
            FormKind::Replace => {
                self.doc.find_text = value(0)?.into();
                self.doc.replace_text = value(1)?.into();
                self.doc.find_case = value(2)? == "true";
                self.doc.find_whole = value(3)? == "true";
                ensure!(!self.doc.find_text.is_empty(), "Enter the text to find");
                let count = self.editor()?.replace_all(
                    &self.doc.find_text,
                    &self.doc.replace_text,
                    self.doc.find_case,
                    self.doc.find_whole,
                )?;
                MessageDialog::new()
                    .set_title("Find and Replace")
                    .set_description(format!("Replaced {count} occurrences."))
                    .show();
            }
            FormKind::GoToLine => {
                let line = value(0)?.parse::<usize>()?;
                ensure!(line > 0, "Line numbers start at 1");
                let text = self.editor()?.text()?;
                let mut offset = 0;
                let mut found = false;
                for (index, part) in text.split_inclusive('\n').enumerate() {
                    if index + 1 == line {
                        found = true;
                        break;
                    }
                    offset += part.encode_utf16().count();
                }
                ensure!(found, "That line does not exist");
                self.editor()?.select_range(offset..offset)?;
            }
        }
        self.editor()?.focus();
        Ok(())
    }
    fn menu_entries(&self, kind: MenuKind) -> Result<Vec<MenuEntry>> {
        use Command as C;
        use MenuKind as M;
        let item = MenuEntry::item;
        let sep = MenuEntry::separator;
        let e =
            |label: &str, shortcut: &str, edit: EditCommand| item(label, shortcut, C::Edit(edit));
        let entries = match kind {
            M::App => vec![
                item("About ExEditor", "", C::About),
                sep(),
                item("Settings…", "Ctrl+,", C::Preferences),
                sep(),
                item("Quit ExEditor", "Ctrl+Q", C::Quit),
            ],
            M::File => vec![
                item("New", "Ctrl+N", C::New),
                item("New Tab", "Ctrl+Shift+N", C::NewTab),
                item("Open…", "Ctrl+O", C::Open),
                item("Open RTFD Folder…", "", C::OpenBundle),
                item("Open Recent", "›", C::Menu(M::Recent)),
                sep(),
                item("Close", "Ctrl+W", C::Close),
                item("Save…", "Ctrl+S", C::Save),
                item("Save As…", "Ctrl+Shift+S", C::SaveAs),
                item("Duplicate", "", C::Duplicate),
                item("Rename…", "", C::Rename),
                item("Move To…", "", C::Move),
                item("Revert to Saved", "", C::Revert),
                item("Browse All Versions…", "", C::Menu(M::Versions)),
                sep(),
                item("Export as PDF…", "", C::Pdf),
                item("Show Properties", "", C::Properties),
                sep(),
                item("Page Setup…", "Ctrl+Shift+P", C::PageSetup),
                item("Print…", "Ctrl+P", C::Print),
            ],
            M::Recent => {
                let mut items = self
                    .prefs
                    .recent_files
                    .iter()
                    .map(|p| {
                        item(
                            &p.file_name().unwrap_or_default().to_string_lossy(),
                            "",
                            C::OpenPath(p.clone()),
                        )
                    })
                    .collect::<Vec<_>>();
                items.push(sep());
                items.push(item("Clear Menu", "", C::ClearRecent));
                items
            }
            M::Versions => {
                if let Some(path) = &self.doc.path {
                    document::list_versions(path)?
                        .into_iter()
                        .map(|p| {
                            item(
                                &p.file_name().unwrap_or_default().to_string_lossy(),
                                "",
                                C::Restore(p.clone()),
                            )
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            M::Edit => vec![
                e("Undo", "Ctrl+Z", EditCommand::Undo),
                e("Redo", "Ctrl+Shift+Z", EditCommand::Redo),
                sep(),
                e("Cut", "Ctrl+X", EditCommand::Cut),
                e("Copy", "Ctrl+C", EditCommand::Copy),
                e("Paste", "Ctrl+V", EditCommand::Paste),
                e(
                    "Paste and Match Style",
                    "Ctrl+Shift+V",
                    EditCommand::PastePlain,
                ),
                e("Delete", "", EditCommand::Delete),
                e("Select All", "Ctrl+A", EditCommand::SelectAll),
                sep(),
                item("Find", "›", C::Menu(M::Find)),
                item("Spelling and Grammar", "›", C::Menu(M::Spelling)),
                item("Substitutions", "›", C::Menu(M::Substitutions)),
                item("Transformations", "›", C::Menu(M::Transformations)),
                sep(),
                item("Start Speaking", "", C::Speak),
                item("Stop Speaking", "", C::StopSpeaking),
                item("Complete", "Esc", C::Complete),
            ],
            M::Find => vec![
                item("Find…", "Ctrl+F", C::Find),
                item("Find and Replace…", "Ctrl+H", C::Replace),
                item("Find Next", "Ctrl+G", C::FindNext),
                item("Find Previous", "Ctrl+Shift+G", C::FindPrevious),
                item("Use Selection for Find", "Ctrl+E", C::UseSelection),
                item("Jump to Selection", "", C::GoToSelection),
                item("Go to Line…", "Ctrl+L", C::GoToLine),
            ],
            M::Spelling => vec![
                item("Show Spelling…", "", C::CheckSpelling),
                item("Check Document Now", "", C::CheckSpelling),
                sep(),
                item("Check Spelling While Typing", "", C::Toggle("spelling"))
                    .checked(self.prefs.spellcheck),
            ],
            M::Substitutions => vec![
                item("Smart Quotes", "", C::Toggle("quotes")).checked(self.prefs.smart_quotes),
                item("Smart Dashes", "", C::Toggle("dashes")).checked(self.prefs.smart_dashes),
                item("Smart Links", "", C::Toggle("links")).checked(self.prefs.automatic_links),
            ],
            M::Transformations => vec![
                e("Make Upper Case", "", EditCommand::ConvertCase(Case::Upper)),
                e("Make Lower Case", "", EditCommand::ConvertCase(Case::Lower)),
                e("Capitalize", "", EditCommand::ConvertCase(Case::Title)),
            ],
            M::Format => vec![
                item("Font", "›", C::Menu(M::Font)),
                item("Text Alignment", "›", C::Menu(M::Alignment)),
                item("Lists…", "›", C::Menu(M::Lists)),
                item("Table…", "", C::Table),
                item("Link…", "", C::Link),
                item("Insert Image…", "", C::Image),
                sep(),
                item(
                    if self.doc.rich {
                        "Make Plain Text"
                    } else {
                        "Make Rich Text"
                    },
                    "Ctrl+Shift+T",
                    C::ToggleFormat,
                ),
                item(
                    if self.doc.wrap_page {
                        "Wrap to Window"
                    } else {
                        "Wrap to Page"
                    },
                    "",
                    C::ToggleWrap,
                ),
                item(
                    if self.doc.read_only {
                        "Unlock Document"
                    } else {
                        "Lock Document"
                    },
                    "",
                    C::ToggleReadOnly,
                ),
            ],
            M::Font => vec![
                item("Show Fonts", "Ctrl+T", C::Font),
                e("Bold", "Ctrl+B", EditCommand::Bold).checked(self.doc.formatting.bold),
                e("Italic", "Ctrl+I", EditCommand::Italic).checked(self.doc.formatting.italic),
                e("Underline", "Ctrl+U", EditCommand::Underline)
                    .checked(self.doc.formatting.underline),
                e("Strikethrough", "", EditCommand::Strikethrough)
                    .checked(self.doc.formatting.strikethrough),
                sep(),
                e("Superscript", "", EditCommand::Superscript),
                e("Subscript", "", EditCommand::Subscript),
                sep(),
                item("Show Colors…", "", C::Foreground),
                item("Text Background Color…", "", C::Background),
                item("Insert Color Code…", "", C::InsertColorCode),
            ],
            M::Alignment => [
                ("Left", Alignment::Left),
                ("Center", Alignment::Center),
                ("Right", Alignment::Right),
                ("Justified", Alignment::Justify),
            ]
            .into_iter()
            .map(|(name, alignment)| {
                e(name, "", EditCommand::Align(alignment))
                    .checked(self.doc.formatting.alignment == alignment)
            })
            .chain([
                sep(),
                e(
                    "Writing Direction: Left to Right",
                    "",
                    EditCommand::Direction(false),
                ),
                e(
                    "Writing Direction: Right to Left",
                    "",
                    EditCommand::Direction(true),
                ),
                e("First Line Indent", "", EditCommand::FirstLineIndent(360)),
                e("Hanging Indent", "", EditCommand::FirstLineIndent(-360)),
                e("Increase Indent", "", EditCommand::Indent(360)),
                e("Decrease Indent", "", EditCommand::Indent(-360)),
            ])
            .collect(),
            M::Lists => vec![
                e("None", "", EditCommand::List(ListKind::None)),
                e("Bulleted List", "", EditCommand::List(ListKind::Bullet)),
                e("Numbered List", "", EditCommand::List(ListKind::Number)),
            ],
            M::View => vec![
                item(
                    if self.show_tabs {
                        "Hide Tab Bar"
                    } else {
                        "Show Tab Bar"
                    },
                    "",
                    C::ToggleTabs,
                ),
                item(
                    if self.doc.ruler {
                        "Hide Ruler"
                    } else {
                        "Show Ruler"
                    },
                    "",
                    C::ToggleRuler,
                ),
                sep(),
                item("Actual Size", "Ctrl+0", C::Zoom(0)),
                item("Zoom In", "Ctrl++", C::Zoom(10)),
                item("Zoom Out", "Ctrl+-", C::Zoom(-10)),
            ],
            M::Window => vec![
                item("New Window", "Ctrl+N", C::New),
                sep(),
                item(&self.name(), "", C::GoToSelection),
            ],
            M::Help => vec![
                item("ExEditor Help", "", C::Help),
                item("About ExEditor", "", C::About),
            ],
            M::Fonts => [
                crate::fonts::RICH_FONT,
                crate::fonts::PLAIN_FONT,
                crate::fonts::INTER_FONT,
                "Arial",
                "Times New Roman",
                "Georgia",
                "Consolas",
                "Courier New",
                "Segoe UI",
            ]
            .into_iter()
            .map(|name| {
                e(name, "", EditCommand::Font(name.into()))
                    .checked(self.doc.formatting.font == name)
            })
            .chain([sep(), item("Show Fonts…", "", C::Font)])
            .collect(),
            M::Sizes => [
                8., 9., 10., 11., 12., 14., 16., 18., 24., 36., 48., 64., 72.,
            ]
            .into_iter()
            .map(|size| {
                e(&format!("{size:.0}"), "", EditCommand::FontSize(size))
                    .checked((self.doc.formatting.size - size).abs() < 0.1)
            })
            .collect(),
            M::Weights => vec![
                e("Regular", "", EditCommand::Bold).checked(!self.doc.formatting.bold),
                e("Bold", "", EditCommand::Bold).checked(self.doc.formatting.bold),
                e("Italic", "", EditCommand::Italic).checked(self.doc.formatting.italic),
            ],
            M::Spacing => [1., 1.15, 1.5, 2., 2.5, 3.]
                .into_iter()
                .map(|spacing| {
                    e(
                        &format!("{spacing:.1}"),
                        "",
                        EditCommand::LineSpacing(spacing),
                    )
                })
                .chain([item("Custom…", "", C::CustomSpacing)])
                .collect(),
        };
        Ok(entries)
    }
    fn toolbar_control(
        id: &'static str,
        label: String,
        width: f32,
        active: bool,
        command: Command,
        colors: Palette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::button::Button {
        ui::control(id, label, width, active, colors).on_click(
            cx.listener(move |this, _, window, cx| this.command(command.clone(), window, cx)),
        )
    }
    fn render_toolbar(&self, c: Palette, cx: &mut Context<Self>) -> AnyElement {
        let font = if self.doc.formatting.font.is_empty() {
            self.prefs.rich_font.clone()
        } else {
            self.doc.formatting.font.clone()
        };
        let size = if self.doc.formatting.size > 0. {
            self.doc.formatting.size
        } else {
            12.
        };
        let mut controls =
            vec![
                Self::toolbar_control(
                    "paragraph-style",
                    "¶⌄".into(),
                    28.,
                    false,
                    Command::Menu(MenuKind::Lists),
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "font-family",
                    font,
                    130.,
                    false,
                    Command::Menu(MenuKind::Fonts),
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "font-face",
                    if self.doc.formatting.bold {
                        "Bold"
                    } else if self.doc.formatting.italic {
                        "Italic"
                    } else {
                        "Regular"
                    }
                    .into(),
                    78.,
                    false,
                    Command::Menu(MenuKind::Weights),
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "font-size",
                    format!("{size:.0} ⌄"),
                    43.,
                    false,
                    Command::Menu(MenuKind::Sizes),
                    c,
                    cx,
                )
                .into_any_element(),
                ui::control("text-color", "", 24., false, c)
                    .child(
                        div()
                            .size(px(12.))
                            .rounded(px(2.))
                            .bg(rgb(self.doc.formatting.foreground)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.command(Command::Foreground, window, cx)
                    }))
                    .into_any_element(),
                Self::toolbar_control(
                    "text-background",
                    "A̲".into(),
                    24.,
                    false,
                    Command::Background,
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "bold",
                    "B".into(),
                    22.,
                    self.doc.formatting.bold,
                    Command::Edit(EditCommand::Bold),
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "italic",
                    "I".into(),
                    22.,
                    self.doc.formatting.italic,
                    Command::Edit(EditCommand::Italic),
                    c,
                    cx,
                )
                .into_any_element(),
                Self::toolbar_control(
                    "underline",
                    "U̲".into(),
                    22.,
                    self.doc.formatting.underline,
                    Command::Edit(EditCommand::Underline),
                    c,
                    cx,
                )
                .into_any_element(),
            ];
        for (id, data, alignment) in [
            ("left", ui::ALIGN_LEFT, Alignment::Left),
            ("center", ui::ALIGN_CENTER, Alignment::Center),
            ("right", ui::ALIGN_RIGHT, Alignment::Right),
            ("justify", ui::ALIGN_JUSTIFY, Alignment::Justify),
        ] {
            let command = Command::Edit(EditCommand::Align(alignment));
            let active = self.doc.formatting.alignment == alignment;
            controls.push(
                ui::control(id, "", 22., active, c)
                    .child(ui::icon(data, if active { 0xffffff } else { c.ink }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.command(command.clone(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        controls.push(
            Self::toolbar_control(
                "line-spacing",
                "1.0 ⌄".into(),
                45.,
                false,
                Command::Menu(MenuKind::Spacing),
                c,
                cx,
            )
            .into_any_element(),
        );
        controls.push(
            ui::control("list", "", 32., false, c)
                .child(ui::icon(ui::LIST, c.ink))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.command(Command::Menu(MenuKind::Lists), window, cx)
                }))
                .into_any_element(),
        );
        div()
            .id("formatting-toolbar")
            .flex()
            .items_center()
            .gap(px(3.))
            .h(px(29.))
            .px(px(7.))
            .flex_shrink_0()
            .bg(rgb(c.toolbar))
            .border_b_1()
            .border_color(rgb(c.border))
            .overflow_hidden()
            .children(controls)
            .into_any_element()
    }
}

#[derive(Default)]
struct OpenEditors(Vec<(AnyWindowHandle, WeakEntity<EditorWindow>)>);
impl Global for OpenEditors {}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.colors(window);
        let dirty = self.doc.dirty;
        let name = self.name();
        let mut lights = div().flex().items_center().gap(px(8.)).px(px(12.)).h_full();
        for (id, color, action) in [
            ("close", 0xff5f57, 0),
            ("minimize", 0xfebc2e, 1),
            ("zoom", 0x28c840, 2),
        ] {
            lights = lights.child(
                div()
                    .id(id)
                    .size(px(12.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(color - 0x101010))
                    .bg(rgb(color))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .when(id == "close" && dirty, |d| {
                        d.child(
                            div()
                                .size(px(4.))
                                .m(px(3.))
                                .rounded_full()
                                .bg(rgb(0x8e221c)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| match action {
                        0 => this.command(Command::CloseWindow, window, cx),
                        1 => window.minimize_window(),
                        _ => window.zoom_window(),
                    })),
            );
        }
        let title = div()
            .id("titlebar")
            .relative()
            .h(px(31.))
            .flex_shrink_0()
            .bg(rgb(c.chrome))
            .flex()
            .items_center()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .on_double_click(|_, window, _| window.zoom_window())
            .child(lights)
            .child(
                div()
                    .absolute()
                    .left(px(100.))
                    .right(px(100.))
                    .text_center()
                    .text_size(px(13.))
                    .child(format!("{name}{}", if dirty { " — Edited" } else { "" })),
            );
        let menubar = div()
            .id("menubar")
            .flex()
            .items_center()
            .h(px(24.))
            .flex_shrink_0()
            .px(px(8.))
            .gap(px(2.))
            .bg(rgb(c.chrome))
            .border_b_1()
            .border_color(rgb(c.border))
            .children(
                [
                    ("ExEditor", MenuKind::App),
                    ("File", MenuKind::File),
                    ("Edit", MenuKind::Edit),
                    ("Format", MenuKind::Format),
                    ("View", MenuKind::View),
                    ("Window", MenuKind::Window),
                    ("Help", MenuKind::Help),
                ]
                .into_iter()
                .map(|(label, kind)| {
                    div()
                        .id(label)
                        .px(px(9.))
                        .h_full()
                        .flex()
                        .items_center()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .when(label == "ExEditor", |d| d.font_weight(FontWeight::SEMIBOLD))
                        .hover(|s| s.bg(rgb(c.border)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.command(Command::Menu(kind.clone()), window, cx)
                        }))
                }),
            );
        let mut root = div()
            .id("exeditor")
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(c.border))
            .bg(rgb(c.paper))
            .text_color(rgb(c.ink))
            .font_family(crate::fonts::RICH_FONT)
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in &paths.0 {
                    this.command(Command::OpenPath(path.clone()), window, cx);
                }
            }))
            .child(title)
            .child(menubar);
        if self.show_tabs || self.tabs.len() > 1 {
            let current = self.active_tab;
            root = root.child(
                div()
                    .id("document-tabs")
                    .flex()
                    .h(px(29.))
                    .flex_shrink_0()
                    .bg(rgb(c.toolbar))
                    .border_b_1()
                    .border_color(rgb(c.border))
                    .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                        let name = tab
                            .as_ref()
                            .map(DocumentTab::name)
                            .unwrap_or_else(|| self.name());
                        ui::control(("tab", index), name, 160., index == current, c).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.command(Command::SwitchTab(index), window, cx)
                            }),
                        )
                    }))
                    .child(
                        ui::control("new-tab", "+", 28., false, c).on_click(cx.listener(
                            |this, _, window, cx| this.command(Command::NewTab, window, cx),
                        )),
                    ),
            );
        }
        if self.doc.rich {
            root = root.child(self.render_toolbar(c, cx));
        }
        if self.doc.rich && self.doc.ruler {
            let editor = self.doc.native.clone();
            root = root.child(ui::ruler(c, self.doc.zoom, move |position, _, _| {
                if let Some(editor) = &editor {
                    let _ = editor.execute(EditCommand::TabStop(
                        ((position - 12.).max(0.) * 15.).round() as i32,
                    ));
                }
            }));
        }
        if let Some(editor) = &self.doc.native {
            let editor = editor.clone();
            let page = self.doc.page.clone();
            let wrap_page = self.doc.wrap_page;
            let zoom = self.doc.zoom;
            editor.set_wrap(true);
            // RichEdit is a child HWND: position it in physical pixels after GPUI layout.
            root = root.child(
                div()
                    .id("document")
                    .flex_1()
                    .min_h(px(1.))
                    .w_full()
                    .bg(rgb(if wrap_page { c.chrome } else { c.paper }))
                    .child(
                        canvas(
                            move |bounds, window, _| {
                                let scale = window.scale_factor();
                                let mut x = f32::from(bounds.origin.x);
                                let y = f32::from(bounds.origin.y);
                                let mut width = f32::from(bounds.size.width);
                                let height = f32::from(bounds.size.height);
                                if wrap_page {
                                    let paper_width =
                                        page.width_twips as f32 / 15. * zoom as f32 / 100.;
                                    if width > paper_width {
                                        x += (width - paper_width) / 2.;
                                        width = paper_width;
                                    }
                                }
                                editor.set_bounds(
                                    (x * scale).round() as i32,
                                    (y * scale).round() as i32,
                                    (width * scale).round() as i32,
                                    (height * scale).round() as i32,
                                );
                                editor.set_background(0xffffff);
                                editor.set_margins(6, 6);
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    ),
            );
        } else {
            root = root.child(div().p(px(24.)).child(
                "The document editor could not be initialized. Close this window and try again.",
            ));
        }
        root
    }
}

impl DocumentTab {
    fn new(prefs: &Preferences, parent: isize) -> Self {
        let native = match NativeEditor::new(parent) {
            Ok(editor) => Some(Rc::new(editor)),
            Err(error) => {
                show_error(&error);
                None
            }
        };
        if let Some(editor) = &native {
            let initialized = editor
                .set_plain_text_mode(!prefs.rich_text)
                .and_then(|()| {
                    editor.execute(EditCommand::Font(if prefs.rich_text {
                        prefs.rich_font.clone()
                    } else {
                        prefs.plain_font.clone()
                    }))
                })
                .and_then(|()| {
                    editor.execute(EditCommand::FontSize(if prefs.rich_text {
                        prefs.rich_size
                    } else {
                        prefs.plain_size
                    }))
                });
            if let Err(error) = initialized {
                show_error(&error);
            }
            editor.set_auto_links(prefs.automatic_links);
            editor.set_spellcheck(prefs.spellcheck);
            editor.mark_saved();
        }
        let recovery_id = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let buffer_lease = match session::lease_buffer(&recovery_id) {
            Ok(lease) => lease,
            Err(error) => {
                show_error(&error);
                None
            }
        };
        Self {
            native,
            path: None,
            format: if prefs.rich_text {
                DocumentFormat::Rtf
            } else {
                DocumentFormat::PlainText
            },
            encoding: prefs.default_encoding,
            line_ending: LineEnding::CrLf,
            fingerprint: None,
            rich: prefs.rich_text,
            dirty: false,
            read_only: false,
            ruler: prefs.show_ruler,
            wrap_page: prefs.wrap_to_page,
            zoom: 100,
            formatting: FormatState::default(),
            page: PageSettings::default(),
            recovery_id,
            buffer_lease,
            buffered: false,
            retain_buffer: true,
            buffer_error_reported: false,
            restored_buffer: false,
            last_recovery: Instant::now(),
            last_external_check: Instant::now(),
            last_text: String::new(),
            find_text: String::new(),
            replace_text: String::new(),
            find_case: false,
            find_whole: false,
        }
    }
    fn restore(prefs: &Preferences, parent: isize, buffer: RestoredBuffer) -> Result<Self> {
        let mut tab = Self::new(prefs, parent);
        let editor = tab
            .native
            .as_ref()
            .context("The native text editor could not be created")?;
        editor.set_plain_text_mode(buffer.document.format.is_plain())?;
        match &buffer.document.content {
            DocumentContent::Plain(text) => editor.set_text(text)?,
            DocumentContent::Rich(bytes) => editor.set_rtf(bytes)?,
        }
        tab.last_text = editor.text()?;
        tab.formatting = editor.formatting();
        editor.acknowledge_buffered_changes();
        tab.path = buffer.document.original_path;
        tab.fingerprint = tab
            .path
            .as_deref()
            .and_then(|path| document::fingerprint(path).ok());
        tab.format = buffer.document.format;
        tab.encoding = buffer.document.encoding;
        tab.line_ending = buffer.document.line_ending;
        tab.rich = !tab.format.is_plain();
        tab.recovery_id = buffer.document.id;
        tab.buffer_lease = Some(buffer.lease);
        tab.dirty = true;
        tab.restored_buffer = true;
        tab.buffered = true;
        Ok(tab)
    }
    fn persist_buffer(&mut self) -> Result<()> {
        if !self.retain_buffer {
            return Ok(());
        }
        let editor = self
            .native
            .as_ref()
            .context("The native editor cannot protect this buffer")?;
        self.dirty |= editor.is_modified();
        if !self.dirty && self.path.is_some() {
            return Ok(());
        }
        if self.buffer_lease.is_none() {
            self.buffer_lease = session::lease_buffer(&self.recovery_id)?;
        }
        ensure!(
            self.buffer_lease.is_some(),
            "Another ExEditor instance owns this unsaved buffer; save a copy before closing."
        );
        let content = if self.rich {
            DocumentContent::Rich(editor.rtf()?)
        } else {
            DocumentContent::Plain(editor.text()?)
        };
        document::write_recovery(&RecoveryDocument {
            id: self.recovery_id.clone(),
            original_path: self.path.clone(),
            format: self.format,
            content,
            encoding: self.encoding,
            line_ending: self.line_ending,
            saved_at: chrono::Local::now().to_rfc3339(),
        })?;
        editor.acknowledge_buffered_changes();
        self.buffered = true;
        self.buffer_error_reported = false;
        self.last_recovery = Instant::now();
        Ok(())
    }
    fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }
}
