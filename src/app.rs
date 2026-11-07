//! Document windows and commands shared by menus, toolbar, and editor shortcuts.
use crate::{
    color::{ColorTarget, Rgba},
    color_picker,
    dialogs::{self, Commands, Field, FormKind, MenuEntry},
    document::{
        self, DocumentContent, DocumentFormat, FileFingerprint, LineEnding, RecoveryDocument,
        TextEncoding,
    },
    editing::LineAction,
    editor::{
        Alignment, Case, EditCommand, Editor, FormatState, ListKind, PageSettings,
        surface::EditorSurface,
    },
    preferences::{Appearance, Preferences},
    services::{self, Speech},
    session::{self, BufferLease, RestoredBuffer},
    syntax::{self, CodeHighlighter},
    ui::{self, Palette, px},
};
use anyhow::{Context as _, Result, bail, ensure};
use gpui_kit::{prelude::FluentBuilder as _, *};
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
    MoveTabs(TransferScope),
    Help,
    Fonts,
    Sizes,
    Weights,
    Spacing,
    Versions,
    InterfaceSize,
    CodeTheme,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferScope {
    CurrentTab,
    AllTabs,
}
#[derive(Clone, Debug)]
pub enum Command {
    New,
    NewTab,
    SwitchTab(usize),
    NextTab(bool),
    MoveTab(bool),
    QuickOpen,
    ToggleTabs,
    CloseWindow,
    MoveTabs {
        target: EntityId,
        scope: TransferScope,
    },
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
    ChoosePrinter {
        printers: Vec<(String, bool)>,
        document: std::sync::Arc<wordcraft_doc::Document>,
        page: PageSettings,
    },
    PrintTo {
        printer: String,
        document: std::sync::Arc<wordcraft_doc::Document>,
        page: PageSettings,
    },
    ReportError(String),
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
    ToggleDarkDocuments,
    CodeTheme(String),
    PreviewCodeTheme(String),
    EndPreview,
    ToggleMarkdown,
    Zoom(i16),
    /// Format > Font > Bigger/Smaller, in points.
    FontStep(f32),
    CustomFontSize,
    /// View > Interface Size, in percent.
    InterfaceScale(u16),
    CustomInterfaceScale,
    Line(LineAction),
    SelectLine,
    MatchingBracket,
    ToggleWordWrap,
    ScrollEditor(i32, bool),
    Palette,
    KeyboardHelp,
    SaveAll,
    CloseAll,
    CopyPath,
    RevealFile,
    PasteOrPreview,
    PasteHtml,
    BlockComment,
    Fullscreen,
    ReopenClosed,
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
    LanguageTools,
    ExplainProofing(String),
    ApplyLanguageTools(crate::proofing::ProofingOptions),
    ProofingCorrection {
        document: String,
        source: std::sync::Arc<str>,
        range: std::ops::Range<usize>,
        original: String,
        replacement: String,
    },
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
/// The command behind a `editor::shortcut` name, shared by the document and window key paths.
pub(crate) fn shortcut_command(name: &str) -> Option<Command> {
    Some(match name {
        "command-palette" => Command::Palette,
        "block-comment" => Command::BlockComment,
        "fullscreen" => Command::Fullscreen,
        "reopen-closed" => Command::ReopenClosed,
        "keyboard-help" => Command::KeyboardHelp,
        "new" => Command::New,
        "open" => Command::Open,
        "quick-open" => Command::QuickOpen,
        "recent-files" => Command::Menu(MenuKind::Recent),
        "complete" => Command::Complete,
        "move-tab-left" => Command::MoveTab(true),
        "move-tab-right" => Command::MoveTab(false),
        "file-menu" => Command::Menu(MenuKind::File),
        "edit-menu" => Command::Menu(MenuKind::Edit),
        "format-menu" => Command::Menu(MenuKind::Format),
        "view-menu" => Command::Menu(MenuKind::View),
        "window-menu" => Command::Menu(MenuKind::Window),
        "help-menu" => Command::Menu(MenuKind::Help),
        "save" => Command::Save,
        "save-as" => Command::SaveAs,
        "save-all" => Command::SaveAll,
        "close-all" => Command::CloseAll,
        "close-window" => Command::CloseWindow,
        "copy-path" => Command::CopyPath,
        "reveal-file" => Command::RevealFile,
        "markdown-preview" => Command::ToggleMarkdown,
        "paste-or-preview" => Command::PasteOrPreview,
        "undo" => Command::Edit(EditCommand::Undo),
        "copy" => Command::Edit(EditCommand::Copy),
        "cut" => Command::Edit(EditCommand::Cut),
        "paste" => Command::Edit(EditCommand::Paste),
        "paste-html" => Command::PasteHtml,
        "select-all" => Command::Edit(EditCommand::SelectAll),
        "select-line" => Command::SelectLine,
        "matching-bracket" => Command::MatchingBracket,
        "word-wrap" => Command::ToggleWordWrap,
        "move-line-up" => Command::Line(LineAction::MoveUp),
        "move-line-down" => Command::Line(LineAction::MoveDown),
        "copy-line-up" => Command::Line(LineAction::CopyUp),
        "copy-line-down" => Command::Line(LineAction::CopyDown),
        "delete-line" => Command::Line(LineAction::Delete),
        "insert-line-above" => Command::Line(LineAction::InsertAbove),
        "insert-line-below" => Command::Line(LineAction::InsertBelow),
        "indent-line" => Command::Line(LineAction::Indent),
        "outdent-line" => Command::Line(LineAction::Outdent),
        "toggle-comment" => Command::Line(LineAction::ToggleComment),
        "add-comment" => Command::Line(LineAction::AddComment),
        "remove-comment" => Command::Line(LineAction::RemoveComment),
        "trim-whitespace" => Command::Line(LineAction::TrimTrailingWhitespace),
        "scroll-line-up" => Command::ScrollEditor(-1, false),
        "scroll-line-down" => Command::ScrollEditor(1, false),
        "scroll-page-up" => Command::ScrollEditor(-1, true),
        "scroll-page-down" => Command::ScrollEditor(1, true),
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
        "bold" => Command::Edit(EditCommand::Bold),
        "italic" => Command::Edit(EditCommand::Italic),
        "underline" => Command::Edit(EditCommand::Underline),
        "redo" => Command::Edit(EditCommand::Redo),
        "paste-plain" => Command::Edit(EditCommand::PastePlain),
        "colors" => Command::Foreground,
        "align-left" => Command::Edit(EditCommand::Align(Alignment::Left)),
        "align-center" => Command::Edit(EditCommand::Align(Alignment::Center)),
        "align-right" => Command::Edit(EditCommand::Align(Alignment::Right)),
        "jump-to-selection" => Command::GoToSelection,
        "ruler" => Command::ToggleRuler,
        "wrap" => Command::ToggleWrap,
        "check-spelling" | "show-spelling" => Command::CheckSpelling,
        "bigger" => Command::FontStep(1.),
        "smaller" => Command::FontStep(-1.),
        _ => return None,
    })
}
fn shortcut_entries() -> Vec<MenuEntry> {
    crate::keyboard::BINDINGS
        .iter()
        .filter_map(|binding| {
            if binding.label.is_empty() {
                return None;
            }
            let command = shortcut_command(binding.action)?;
            let mut label = match binding.action {
                "new" => "New window".into(),
                "paste-or-preview" => "Paste plain text / Markdown preview".into(),
                _ => binding.action.replace('-', " "),
            };
            if let Some(first) = label.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            Some(MenuEntry::item(&label, binding.label, command))
        })
        .collect()
}
/// Dark-document background and the light stand-in for automatic text.
const DARK_PAPER: u32 = 0x1e1e1e;
const DARK_TEXT: u32 = 0xe6e6e5;
/// Common source extensions for the Open dialog's filter; any file opens with All Files.
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs",
    "c",
    "h",
    "cpp",
    "cc",
    "cxx",
    "hpp",
    "cs",
    "java",
    "kt",
    "kts",
    "swift",
    "go",
    "py",
    "pyw",
    "rb",
    "php",
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "tsx",
    "json",
    "jsonc",
    "toml",
    "yaml",
    "yml",
    "xml",
    "css",
    "scss",
    "less",
    "sql",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "lua",
    "pl",
    "r",
    "dart",
    "scala",
    "hs",
    "ex",
    "exs",
    "erl",
    "clj",
    "fs",
    "vb",
    "zig",
    "nim",
    "jl",
    "tex",
    "ini",
    "cfg",
    "conf",
    "log",
    "diff",
    "patch",
    "dockerfile",
    "makefile",
    "cmake",
    "gradle",
    "vue",
    "svelte",
    "graphql",
    "proto",
    "tf",
    "csv",
];
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn"
            )
        })
}
fn comment_prefix(path: Option<&Path>) -> &'static str {
    match path
        .and_then(Path::extension)
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "py" | "pyw" | "rb" | "sh" | "bash" | "zsh" | "ps1" | "psm1" | "yml" | "yaml" | "toml"
        | "r" | "pl" => "#",
        "sql" | "lua" | "hs" => "--",
        "bat" | "cmd" => "REM",
        "tex" | "erl" => "%",
        _ => "//",
    }
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

/// Routes launch requests into existing document windows instead of starting more windows.
pub fn open_paths(paths: Vec<PathBuf>, cx: &mut App) {
    let editors = cx
        .try_global::<OpenEditors>()
        .map(|open| open.0.clone())
        .unwrap_or_default();
    if paths.is_empty() {
        if let Some((handle, editor)) = editors
            .iter()
            .rev()
            .find(|(_, editor)| editor.upgrade().is_some())
        {
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                let _ = editor.update(cx, |view, _| {
                    if let Some(editor) = &view.doc.editor {
                        editor.focus();
                    }
                });
            });
            cx.activate(true);
        } else {
            open_document(None, cx);
        }
        return;
    }
    for path in paths {
        let editors = cx
            .try_global::<OpenEditors>()
            .map(|open| open.0.clone())
            .unwrap_or_default();
        let existing = editors.iter().find_map(|(handle, editor)| {
            let view = editor.upgrade()?;
            let index = view.read(cx).tab_for_path(&path)?;
            Some((*handle, editor.clone(), index))
        });
        if let Some((handle, editor, index)) = existing {
            let result = handle.update(cx, |_, window, cx| {
                editor.update(cx, |view, cx| {
                    view.switch_tab(index)?;
                    window.activate_window();
                    cx.notify();
                    Ok::<_, anyhow::Error>(())
                })?
            });
            if let Err(error) = result.and_then(|result| result) {
                show_error(&error);
            }
        } else if let Some((handle, editor)) = editors
            .iter()
            .rev()
            .find(|(_, editor)| editor.upgrade().is_some())
        {
            let result = handle.update(cx, |_, window, cx| {
                editor.update(cx, |view, cx| {
                    view.open_file_tab(&path, window, cx)?;
                    window.activate_window();
                    cx.notify();
                    Ok::<_, anyhow::Error>(())
                })?
            });
            if let Err(error) = result.and_then(|result| result) {
                show_error(&error);
            }
        } else {
            open_document(Some(path), cx);
        }
    }
    cx.activate(true);
}

struct FileRequests {
    instance: Rc<crate::instance::Instance>,
    os_requests: Rc<RefCell<Vec<Vec<PathBuf>>>>,
    pending: Vec<Vec<PathBuf>>,
}
impl Global for FileRequests {}

pub fn listen_for_files(
    instance: Rc<crate::instance::Instance>,
    os_requests: Rc<RefCell<Vec<Vec<PathBuf>>>>,
    cx: &mut App,
) {
    cx.set_global(FileRequests {
        instance,
        os_requests,
        pending: Vec::new(),
    });
}
fn dispatch_file_requests(cx: &mut App) {
    let pending = std::mem::take(&mut cx.global_mut::<FileRequests>().pending);
    for paths in pending {
        open_paths(paths, cx);
    }
}
fn prepare_instance_exit(cx: &mut App) -> bool {
    let Some(instance) = cx
        .try_global::<FileRequests>()
        .map(|requests| requests.instance.clone())
    else {
        return true;
    };
    let received = instance.stop_accepting();
    let requests = cx.global_mut::<FileRequests>();
    requests.pending.extend(received);
    let os_requests = std::mem::take(&mut *requests.os_requests.borrow_mut());
    requests.pending.extend(os_requests);
    if requests.pending.is_empty() {
        return true;
    }
    instance.resume_accepting();
    cx.defer(dispatch_file_requests);
    false
}
fn last_document_window(cx: &App) -> bool {
    cx.try_global::<OpenEditors>().is_none_or(|open| {
        open.0
            .iter()
            .filter(|(_, editor)| editor.upgrade().is_some())
            .count()
            <= 1
    })
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
            let mut count = 0;
            for buffer in restored.buffers {
                let result = DocumentTab::restore(&view.prefs, buffer);
                match result {
                    Ok(tab) => {
                        if count == 0 {
                            view.doc = tab;
                        } else {
                            if let Some(editor) = &tab.editor {
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
            if let Some(editor) = &view.doc.editor {
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
    editor: Option<Rc<Editor>>,
    path: Option<PathBuf>,
    format: DocumentFormat,
    encoding: TextEncoding,
    line_ending: LineEnding,
    fingerprint: Option<FileFingerprint>,
    observed_fingerprint: Option<FileFingerprint>,
    rich: bool,
    dirty: bool,
    read_only: bool,
    ruler: bool,
    wrap_page: bool,
    word_wrap: bool,
    untitled_name: String,
    zoom: u16,
    formatting: FormatState,
    page: PageSettings,
    recovery_id: String,
    buffer_lease: Option<std::sync::Arc<BufferLease>>,
    pending_buffer: Option<std::sync::mpsc::Receiver<Result<()>>>,
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
    /// Source-code coloring for plain documents whose type is recognized.
    highlighter: Option<CodeHighlighter>,
    /// Dark paper and code theme last applied; `None` forces a refresh.
    look: Option<(bool, String)>,
    /// The editor's background color.
    paper: u32,
    /// Markdown shown formatted (read-only); this is the text that is saved.
    markdown_source: Option<Rc<Editor>>,
    proofing: crate::language_tools::CheckState,
}
struct EditorWindow {
    doc: DocumentTab,
    tabs: Vec<Option<DocumentTab>>,
    active_tab: usize,
    show_tabs: bool,
    queue: Commands,
    prefs: Preferences,
    speech: Speech,
    was_active: bool,
    /// Code theme shown while browsing the Code Theme menu; `None` shows the saved one.
    preview_theme: Option<String>,
    /// Horizontal scroll of the document tab strip.
    tab_scroll: ScrollHandle,
    chord: crate::keyboard::State,
    focus: Option<FocusHandle>,
    surface: Option<(Rc<Editor>, Entity<EditorSurface>)>,
    _poll: Task<()>,
}

impl EditorWindow {
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Preferences::load().unwrap_or_else(|error| {
            show_error(&error);
            Preferences::default()
        });
        let doc = DocumentTab::new(&prefs);
        let mut this = Self {
            doc,
            tabs: vec![None],
            active_tab: 0,
            show_tabs: false,
            queue: Rc::new(RefCell::new(VecDeque::new())),
            prefs,
            speech: Speech::default(),
            was_active: false,
            preview_theme: None,
            tab_scroll: ScrollHandle::new(),
            chord: crate::keyboard::State::default(),
            focus: None,
            surface: None,
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
        if !cx.has_global::<RecentlyClosed>() {
            cx.set_global(RecentlyClosed::default());
        }
        let registration = (Window::window_handle(window), cx.entity().downgrade());
        cx.global_mut::<OpenEditors>().0.push(registration);
        // Hidden helper windows (menus, color panel) must not keep the process alive.
        let own = cx.entity().entity_id();
        cx.on_release(move |_, cx| {
            let editors = &mut cx.global_mut::<OpenEditors>().0;
            editors.retain(|(_, editor)| editor.entity_id() != own && editor.upgrade().is_some());
            if editors.is_empty() {
                cx.quit();
            }
        })
        .detach();
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.confirm_close(window, cx))
                .unwrap_or(true)
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        this.focus = Some(focus);
        if let Some(editor) = &this.doc.editor {
            editor.focus();
        }
        let handle = Window::window_handle(window);
        this._poll = cx.spawn(async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(100)).await;
                // A modal dialog's message loop can run this while the window is busy;
                // skip that tick instead of stopping, or queued menu commands never run.
                let _ = handle.update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| this.tick(window, cx))
                });
                if this.upgrade().is_none() {
                    break;
                }
            }
        });
        this
    }
    fn editor(&self) -> Result<Rc<Editor>> {
        self.doc
            .editor
            .clone()
            .context("The document editor could not be created")
    }
    fn is_dark(&self, window: &Window) -> bool {
        match self.prefs.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }
    fn colors(&self, window: &Window) -> Palette {
        Palette::new(self.is_dark(window))
    }
    /// Applies the document background, text color, and code coloring when the
    /// appearance, the dark-document preference, or the code theme changed.
    fn refresh_look(&mut self, window: &Window) -> Result<()> {
        let editor = self.editor()?;
        editor.set_autocomplete_enabled(self.prefs.autocomplete);
        let dark_paper = self.is_dark(window) && self.prefs.dark_documents;
        let theme = self
            .preview_theme
            .as_ref()
            .unwrap_or(&self.prefs.code_theme)
            .clone();
        let key = (dark_paper, theme.clone());
        if self.doc.look.as_ref() == Some(&key) {
            return Ok(());
        }
        // Set first so a failure is reported once, not on every tick.
        self.doc.look = Some(key);
        let editor = self.editor()?;
        let syntax = if self.doc.rich || self.doc.markdown_source.is_some() {
            None
        } else {
            self.doc.path.as_deref().and_then(|path| {
                syntax::detect(path, self.doc.last_text.lines().next().unwrap_or(""))
            })
        };
        if self.doc.highlighter.take().is_some() {
            editor.clear_spans()?;
        }
        editor.set_completion_language(syntax.clone());
        self.doc.highlighter =
            syntax.map(|syntax| CodeHighlighter::new(syntax, &theme, dark_paper));
        let (paper, text) = match &self.doc.highlighter {
            Some(highlighter) => {
                let (background, foreground) = highlighter.colors();
                (background, Some(foreground))
            }
            None if dark_paper => (DARK_PAPER, Some(DARK_TEXT)),
            None => (0xffffff, None),
        };
        editor.set_background(paper);
        editor.set_text_override(text)?;
        self.doc.paper = paper;
        self.highlight(&editor)
    }
    /// Colors the lines that changed since the last call.
    fn highlight(&mut self, editor: &Editor) -> Result<()> {
        if let Some(highlighter) = &mut self.doc.highlighter
            && self.doc.last_text.len() <= syntax::MAX_BYTES
        {
            editor.apply_spans(&highlighter.all_spans(&self.doc.last_text))?;
        }
        Ok(())
    }
    /// Shows a Markdown document formatted and read-only, keeping its source for saving.
    fn show_markdown(&mut self, source: String) -> Result<()> {
        let directory = self
            .doc
            .path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        let rtf = crate::formats::markdown_to_rtf(&source, directory)?;
        let editor = self.editor()?;
        let preview = Rc::new(editor.preview(&rtf)?);
        // Keep the original editor buffer alive: text, undo, selection and dirty state.
        self.doc.dirty |= editor.is_modified();
        self.doc.last_text = preview.text()?;
        editor.set_visible(false);
        self.doc.markdown_source = Some(editor);
        preview.set_visible(true);
        self.doc.editor = Some(preview);
        self.doc.look = None;
        Ok(())
    }
    fn show_markdown_source(&mut self) -> Result<()> {
        let Some(source) = self.doc.markdown_source.as_ref() else {
            return Ok(());
        };
        let text = source.text()?;
        source.set_read_only(self.doc.read_only);
        self.doc.dirty |= source.is_modified();
        if let Some(preview) = &self.doc.editor {
            preview.set_visible(false);
        }
        self.doc.editor = self.doc.markdown_source.take();
        self.doc.last_text = text;
        self.doc.look = None;
        if let Some(source) = &self.doc.editor {
            source.set_visible(true);
            source.focus();
        }
        Ok(())
    }
    fn name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc.untitled_name.clone())
    }
    fn content(&self) -> Result<DocumentContent> {
        let editor = self.editor()?;
        if let Some(source) = &self.doc.markdown_source {
            Ok(DocumentContent::Plain(source.text()?))
        } else if self.doc.rich {
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
    fn load_file(&self, path: &Path) -> Result<document::LoadedDocument> {
        let format = DocumentFormat::from_path(path)?;
        ensure!(
            document::supports_format(format),
            "This document format is not supported. Save it as RTF or DOCX in the originating application."
        );
        document::load(path).or_else(|error| {
            if format.is_plain() && self.prefs.default_encoding != TextEncoding::Utf8 {
                document::load_with_encoding(path, self.prefs.default_encoding)
            } else {
                Err(error)
            }
        })
    }
    fn read_file(&mut self, path: &Path) -> Result<()> {
        let loaded = self.load_file(path)?;
        self.load_document(loaded)
    }
    fn load_document(&mut self, loaded: document::LoadedDocument) -> Result<()> {
        let markdown = is_markdown(&loaded.path);
        self.doc.markdown_source = None;
        self.editor()?.set_read_only(self.doc.read_only);
        self.editor()?
            .set_plain_text_mode(loaded.format.is_plain())?;
        if let Some(snapshot) = loaded.editor_snapshot {
            self.editor()?.set_document(snapshot)?;
            self.editor()?.mark_modified();
        } else {
            self.load_content(&loaded.content)?;
        }
        self.doc.page = self.editor()?.page_settings();
        if loaded.format.is_plain() {
            // Plain files have no fonts of their own; without this they keep the rich default.
            let editor = self.editor()?;
            editor.execute(EditCommand::Font(self.prefs.plain_font.clone()))?;
            editor.execute(EditCommand::FontSize(self.prefs.plain_size))?;
        }
        self.doc.rich = !loaded.format.is_plain();
        self.doc.path = Some(loaded.path.clone());
        self.doc.format = loaded.format;
        self.doc.encoding = loaded.encoding;
        self.doc.line_ending = loaded.line_ending;
        self.doc.fingerprint = Some(loaded.fingerprint);
        self.doc.observed_fingerprint = None;
        self.doc.dirty = false;
        self.editor()?.mark_saved();
        self.doc.discard_recovery()?;
        self.doc.buffered = false;
        self.doc.restored_buffer = false;
        self.prefs.add_recent(loaded.path);
        self.prefs.save()?;
        self.doc.last_text = self.editor()?.text()?;
        self.doc.proofing.invalidate();
        self.doc.look = None;
        if let DocumentContent::Plain(source) = loaded.content
            && markdown
            && let Err(error) = self.show_markdown(source)
        {
            show_error(&error.context("The Markdown is shown as source text"));
            self.show_markdown_source()?;
        }
        Ok(())
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(requests) = cx.try_global::<FileRequests>() {
            let mut pending = requests.instance.take_requests();
            pending.extend(std::mem::take(&mut *requests.os_requests.borrow_mut()));
            if !pending.is_empty() {
                // Keep staged requests visible to the close guard until the entity update ends.
                cx.global_mut::<FileRequests>().pending.extend(pending);
                App::defer(cx, dispatch_file_requests);
            }
        }
        while let Some(command) = { self.queue.borrow_mut().pop_front() } {
            self.command(command, window, cx);
        }
        let Some(editor) = self.doc.editor.clone() else {
            return;
        };
        while let Some(shortcut) = editor.take_shortcut() {
            if let Some(command) = shortcut_command(&shortcut) {
                self.command(command, window, cx);
            }
        }
        let Some(editor) = self.doc.editor.clone() else {
            return;
        };
        let active = window.is_window_active();
        let activated = active && !self.was_active;
        if activated {
            let own = cx.entity().entity_id();
            let editors = &mut cx.global_mut::<OpenEditors>().0;
            if let Some(index) = editors
                .iter()
                .position(|(_, editor)| editor.entity_id() == own)
            {
                let entry = editors.remove(index);
                editors.push(entry);
            }
        }
        // Another window may have changed a preference.
        if active
            && !self.was_active
            && let Ok(prefs) = Preferences::load()
        {
            self.prefs = prefs;
        }
        self.was_active = active;
        if activated {
            editor.restore_focus();
        }
        if !active {
            self.chord.reset();
        }
        let dark = self.is_dark(window);
        if gpui_kit::component::Theme::global(cx).mode.is_dark() != dark {
            gpui_kit::component::Theme::change(
                if dark {
                    gpui_kit::component::ThemeMode::Dark
                } else {
                    gpui_kit::component::ThemeMode::Light
                },
                None,
                cx,
            );
        }
        let composing = self
            .surface
            .as_ref()
            .is_some_and(|(_, surface)| surface.read(cx).is_composing());
        self.doc.dirty = editor.is_modified() || self.doc.dirty;
        self.doc.formatting = editor.formatting();
        if editor.take_text_changed()
            && let Ok(mut text) = editor.text()
            && text != self.doc.last_text
        {
            let range = editor.selection();
            if active
                && !composing
                && !self.doc.read_only
                && range.start == range.end
                && text.chars().count() == self.doc.last_text.chars().count() + 1
                && let Some(change) = services::substitution(
                    &text,
                    range.end,
                    self.prefs.smart_quotes,
                    self.prefs.smart_dashes,
                )
                && editor
                    .select_range(change.range)
                    .and_then(|()| editor.replace_current(&change.replacement))
                    .is_ok()
            {
                text = editor.text().unwrap_or(text);
            }
            self.doc.last_text = text;
            self.doc.proofing.invalidate();
            let recolored = if self.doc.highlighter.is_some() {
                self.highlight(&editor)
            } else {
                editor.adapt_automatic_text()
            };
            if let Err(error) = recolored {
                self.doc.highlighter = None;
                show_error(&error.context("Code coloring is turned off for this document"));
            }
        }
        let _ = editor.take_pasted();
        if let Err(error) = self.refresh_look(window) {
            show_error(&error);
        }
        if let Err(error) = self.update_proofing(None, window, cx) {
            show_error(&error);
        }
        for tab in std::iter::once(&mut self.doc).chain(self.tabs.iter_mut().flatten()) {
            if let Err(error) = tab.poll_buffer() {
                if !tab.buffer_error_reported {
                    show_error(&error);
                }
                tab.buffered = false;
                tab.buffer_error_reported = true;
                tab.last_recovery = Instant::now();
            }
        }
        let needs_buffer = editor.has_buffered_changes()
            || (!self.doc.buffered && (self.doc.dirty || self.doc.path.is_none()))
            || (self.doc.dirty
                && self.doc.last_recovery.elapsed()
                    >= Duration::from_secs(self.prefs.autosave_seconds.max(5)));
        if needs_buffer
            && (!self.doc.buffer_error_reported
                || self.doc.last_recovery.elapsed() >= Duration::from_secs(5))
            && let Err(error) = self.doc.queue_buffer()
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
                && self.doc.observed_fingerprint.as_ref() != Some(&new)
            {
                let path = path.clone();
                if !self.doc.dirty {
                    if confirm(
                        "Document changed on disk",
                        "Reload the changes made by another application?",
                    ) {
                        if let Err(error) = self.read_file(&path) {
                            show_error(&error);
                        }
                    } else {
                        // Suppress repeated prompts without accepting unseen disk contents.
                        self.doc.observed_fingerprint = Some(new);
                    }
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
    fn update_proofing(
        &mut self,
        immediate: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if self.doc.markdown_source.is_some() {
            return Ok(());
        }
        let text = immediate.unwrap_or(&self.doc.last_text);
        if text.len() > crate::proofing::MAX_TEXT_BYTES {
            if std::mem::take(&mut self.doc.proofing.manual) {
                bail!("Language tools support documents up to 1 MiB. Check a smaller document.");
            }
            return Ok(());
        }
        let options = &self.prefs.proofing;
        let completed = self.doc.proofing.poll(text, options);
        let current = self.doc.proofing.current(text, options);
        if completed && self.prefs.spellcheck {
            let ranges: Vec<_> = self
                .doc
                .proofing
                .issues
                .iter()
                .map(|issue| issue.range.clone())
                .collect();
            self.editor()?.set_proofing_marks(&ranges)?;
        }
        if !current
            && !self.doc.proofing.pending()
            && self.doc.proofing.ready()
            && (self.doc.proofing.manual || (self.prefs.spellcheck && !self.doc.read_only))
        {
            self.doc
                .proofing
                .start(std::sync::Arc::from(text), options.clone());
        }
        if current && std::mem::take(&mut self.doc.proofing.manual) {
            self.show_proofing_result(window, cx)?;
        }
        Ok(())
    }
    fn show_proofing_result(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        if let Some(error) = &self.doc.proofing.error {
            bail!("{error}");
        }
        let editor = self.editor()?;
        let caret = editor.selection().end;
        let Some(issue) = self
            .doc
            .proofing
            .issues
            .iter()
            .find(|issue| issue.range.start >= caret)
            .or(self.doc.proofing.issues.first())
        else {
            MessageDialog::new()
                .set_title("Spelling and Grammar")
                .set_description("No spelling or grammar suggestions found.")
                .show();
            return Ok(());
        };
        let source = self
            .doc
            .proofing
            .snapshot()
            .context("Check the document again")?;
        ensure!(
            editor.text()?.as_str() == source.as_ref(),
            "The document changed. Check it again."
        );
        editor.select_range(issue.range.clone())?;
        let kind = match issue.kind {
            crate::proofing::IssueKind::Grammar => "Grammar",
            crate::proofing::IssueKind::Spelling => "Spelling",
        };
        let mut description =
            MenuEntry::item(&format!("{kind} suggestion"), "", Command::CheckSpelling);
        description.enabled = false;
        let mut entries = vec![description, MenuEntry::separator()];
        entries.extend(issue.suggestions.iter().map(|replacement| {
            MenuEntry::item(
                if replacement.is_empty() {
                    "Delete"
                } else {
                    replacement
                },
                "",
                Command::ProofingCorrection {
                    document: self.doc.recovery_id.clone(),
                    source: source.clone(),
                    range: issue.range.clone(),
                    original: issue.original.clone(),
                    replacement: replacement.clone(),
                },
            )
        }));
        entries.push(MenuEntry::separator());
        entries.push(MenuEntry::item(
            "Explain Suggestion…",
            "",
            Command::ExplainProofing(issue.message.clone()),
        ));
        entries.push(MenuEntry::item(
            "Next Suggestion",
            "",
            Command::CheckSpelling,
        ));
        entries.push(MenuEntry::item(
            "Language Tools…",
            "",
            Command::LanguageTools,
        ));
        self.show_menu(entries, window, cx);
        Ok(())
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
            && self.doc.markdown_source.is_none()
            && !confirm(
                "Save as plain text?",
                "Plain text removes all formatting, tables, and images. Save a plain-text copy?",
            )
        {
            return Ok(false);
        }
        let content = if format.is_plain()
            && let Some(source) = &self.doc.markdown_source
        {
            DocumentContent::Plain(source.text()?)
        } else if format.is_plain() {
            DocumentContent::Plain(self.editor()?.text()?)
        } else {
            DocumentContent::Rich(self.editor()?.rtf()?)
        };
        let snapshot = self.editor()?.snapshot();
        let fingerprint = if format.is_plain() {
            document::save(
                &path,
                format,
                &content,
                self.doc.encoding,
                self.doc.line_ending,
            )?
        } else {
            document::save_with_document(
                &path,
                format,
                &content,
                self.doc.encoding,
                self.doc.line_ending,
                Some(&snapshot),
            )?
        };
        self.doc.path = Some(path.clone());
        self.doc.format = format;
        self.doc.fingerprint = Some(fingerprint);
        self.doc.observed_fingerprint = None;
        self.doc.rich = !format.is_plain();
        if !format.is_plain() && self.doc.markdown_source.take().is_some() {
            // Save As rich text deliberately keeps the rendered document as the new editable file.
            self.editor()?.set_read_only(self.doc.read_only);
        }
        if self.doc.markdown_source.is_none() {
            self.editor()?.set_plain_text_mode(format.is_plain())?;
        }
        // A new name can mean a different language.
        self.doc.look = None;
        self.doc.dirty = false;
        self.editor()?.mark_saved();
        self.doc.discard_recovery()?;
        self.doc.buffered = false;
        self.doc.restored_buffer = false;
        self.prefs.add_recent(path);
        self.prefs.save()?;
        Ok(true)
    }
    fn confirm_current(&mut self) -> Result<bool> {
        self.doc.dirty |= self.editor()?.is_modified();
        if !self.doc.dirty {
            self.doc.discard_recovery()?;
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
                self.doc.discard_recovery()?;
                true
            }
            MessageDialogResult::Custom(ref label) if label == "Discard" => {
                self.doc.discard_recovery()?;
                true
            }
            _ => false,
        };
        if saved {
            self.doc.retain_buffer = false;
        }
        Ok(saved)
    }
    fn confirm_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let result = self.doc.persist_buffer().and_then(|()| {
            for tab in self.tabs.iter_mut().flatten() {
                tab.persist_buffer()?;
            }
            Ok(())
        });
        match result {
            Ok(()) => !last_document_window(cx) || prepare_instance_exit(cx),
            Err(error) => {
                confirm(
                    "Unsaved changes could not be protected",
                    &format!(
                        "{error:#}\n\nClose anyway and lose the unprotected changes? Choose Cancel to keep the window open and save the document."
                    ),
                ) && (!last_document_window(cx) || prepare_instance_exit(cx))
            }
        }
    }
    fn switch_tab(&mut self, index: usize) -> Result<()> {
        ensure!(
            index < self.tabs.len(),
            "That document tab is no longer open"
        );
        self.tab_scroll.scroll_to_item(index);
        if index == self.active_tab {
            return Ok(());
        }
        self.doc.queue_buffer()?;
        let next = self.tabs[index]
            .take()
            .context("The document tab is unavailable")?;
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(false);
        }
        let previous = std::mem::replace(&mut self.doc, next);
        self.tabs[self.active_tab] = Some(previous);
        self.active_tab = index;
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(true);
            editor.focus();
        }
        Ok(())
    }
    fn new_tab(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Result<()> {
        let doc = DocumentTab::new(&self.prefs);
        if let Some(editor) = &doc.editor {
            editor.set_visible(false);
        }
        self.tabs.push(Some(doc));
        self.show_tabs = true;
        self.switch_tab(self.tabs.len() - 1)
    }
    fn tab_for_path(&self, path: &Path) -> Option<usize> {
        let canonical = std::fs::canonicalize(path).ok()?;
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            let tab = if index == self.active_tab {
                &self.doc
            } else {
                tab.as_ref()?
            };
            (tab.path
                .as_ref()
                .and_then(|path| std::fs::canonicalize(path).ok())
                .as_ref()
                == Some(&canonical))
            .then_some(index)
        })
    }
    fn open_file_tab(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if let Some(index) = self.tab_for_path(path) {
            return self.switch_tab(index);
        }
        // Verify that the path can be read before creating a new tab or touching existing text.
        let loaded = self.load_file(path)?;
        self.new_tab(window, cx)?;
        self.load_document(loaded)
    }
    fn take_transfer(&mut self, scope: TransferScope) -> (Vec<DocumentTab>, usize) {
        if scope == TransferScope::AllTabs || self.tabs.len() == 1 {
            let current = std::mem::replace(&mut self.doc, DocumentTab::vacant(&self.prefs));
            let active = self.active_tab;
            self.tabs[active] = Some(current);
            let tabs = std::mem::replace(&mut self.tabs, vec![None]);
            self.active_tab = 0;
            return (tabs.into_iter().flatten().collect(), active);
        }
        let next = if self.active_tab == 0 {
            1
        } else {
            self.active_tab - 1
        };
        let replacement = self.tabs[next]
            .take()
            .expect("inactive tabs own their documents");
        let moved = std::mem::replace(&mut self.doc, replacement);
        let old = self.active_tab;
        self.tabs.remove(old);
        self.active_tab = if next > old { next - 1 } else { next };
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(true);
            editor.focus();
        }
        (vec![moved], 0)
    }
    fn receive_transfer(&mut self, mut incoming: Vec<DocumentTab>, active: usize) {
        let next = incoming.remove(active);
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(false);
        }
        let previous = std::mem::replace(&mut self.doc, next);
        self.tabs[self.active_tab] = Some(previous);
        self.active_tab = self.tabs.len() + active;
        for (index, tab) in incoming.into_iter().enumerate() {
            if index == active {
                self.tabs.push(None);
            }
            self.tabs.push(Some(tab));
        }
        if self.tabs.len() == self.active_tab {
            self.tabs.push(None);
        }
        self.show_tabs = true;
        self.tab_scroll.scroll_to_item(self.active_tab);
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(true);
            editor.focus();
        }
    }
    fn move_tabs(
        &mut self,
        target: EntityId,
        scope: TransferScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        ensure!(
            target != cx.entity().entity_id(),
            "Choose another document window"
        );
        let (handle, destination) = cx
            .global::<OpenEditors>()
            .0
            .iter()
            .find(|(_, editor)| editor.entity_id() == target && editor.upgrade().is_some())
            .cloned()
            .context("The destination window has closed; your documents have not moved")?;
        let close_source = scope == TransferScope::AllTabs || self.tabs.len() == 1;
        handle.update(cx, |_, target_window, cx| {
            destination.update(cx, |destination, cx| {
                destination.doc.queue_buffer()?;
                let (incoming, active) = self.take_transfer(scope);
                destination.receive_transfer(incoming, active);
                target_window.activate_window();
                cx.notify();
                Ok::<_, anyhow::Error>(())
            })?
        })??;
        if close_source {
            window.remove_window();
        }
        cx.activate(true);
        Ok(())
    }
    fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        if !self.confirm_current()? {
            return Ok(());
        }
        if let Some(path) = self.doc.path.clone() {
            let closed = &mut cx.global_mut::<RecentlyClosed>().0;
            closed.push(path);
            if closed.len() > 20 {
                closed.remove(0);
            }
        }
        if self.tabs.len() == 1 {
            if !last_document_window(cx) || prepare_instance_exit(cx) {
                window.remove_window();
            }
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
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(false);
        }
        self.doc = doc;
        let old = self.active_tab;
        self.tabs.remove(old);
        self.active_tab = if next > old { next - 1 } else { next };
        if let Some(editor) = &self.doc.editor {
            editor.set_visible(true);
            editor.focus();
        }
        Ok(())
    }
    fn reorder_tab(&mut self, left: bool) {
        let target = if left {
            self.active_tab.checked_sub(1)
        } else {
            self.active_tab
                .checked_add(1)
                .filter(|index| *index < self.tabs.len())
        };
        if let Some(target) = target {
            self.tabs.swap(self.active_tab, target);
            self.active_tab = target;
            self.tab_scroll.scroll_to_item(target);
        }
    }
    fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.try_command(command, window, cx) {
            show_error(&error);
        }
        if let Some(editor) = &self.doc.editor {
            editor.restore_focus();
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
            Command::ReopenClosed => {
                if let Some(path) = cx.global_mut::<RecentlyClosed>().0.pop() {
                    self.open_file_tab(&path, window, cx)?;
                }
            }
            Command::Fullscreen => window.toggle_fullscreen(),
            Command::BlockComment => {
                ensure!(
                    self.doc.markdown_source.is_none(),
                    "Choose Show Markdown Source before editing the preview."
                );
                let editor = self.editor()?;
                let text = editor.text()?;
                let extension = self
                    .doc
                    .path
                    .as_deref()
                    .and_then(Path::extension)
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    extension.as_str(),
                    "py" | "rb" | "sh" | "ps1" | "yml" | "yaml" | "toml" | "r" | "sql"
                ) {
                    self.try_command(Command::Line(LineAction::ToggleComment), window, cx)?;
                } else {
                    let (open, close) = if matches!(
                        extension.as_str(),
                        "html" | "htm" | "xml" | "md" | "markdown"
                    ) {
                        ("<!--", "-->")
                    } else if extension == "lua" {
                        ("--[[", "]]")
                    } else {
                        ("/*", "*/")
                    };
                    if let Some(edit) = crate::editing::toggle_block_comment(
                        &text,
                        editor.selection(),
                        open,
                        close,
                    )? {
                        editor.edit_lines(edit)?;
                    }
                }
            }
            Command::Line(action) => {
                ensure!(
                    self.doc.markdown_source.is_none(),
                    "Choose Show Markdown Source before editing the preview."
                );
                let editor = self.editor()?;
                let text = editor.text()?;
                if action == LineAction::ToggleComment
                    && self
                        .doc
                        .path
                        .as_deref()
                        .and_then(Path::extension)
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| {
                            matches!(
                                e.to_ascii_lowercase().as_str(),
                                "html" | "htm" | "xml" | "md" | "markdown"
                            )
                        })
                {
                    return self.try_command(Command::BlockComment, window, cx);
                }
                let prefix = comment_prefix(self.doc.path.as_deref());
                if let Some(edit) = crate::editing::plan(&text, editor.selection(), action, prefix)?
                {
                    editor.edit_lines(edit)?;
                }
            }
            Command::SelectLine => {
                let editor = self.editor()?;
                editor.select_range(crate::editing::select_line(
                    &editor.text()?,
                    editor.selection(),
                )?)?;
            }
            Command::MatchingBracket => {
                let editor = self.editor()?;
                if let Some(position) =
                    crate::editing::matching_bracket(&editor.text()?, editor.selection().end)
                {
                    editor.select_range(position..position)?;
                    editor.scroll_to_selection();
                }
            }
            Command::ToggleWordWrap => self.doc.word_wrap = !self.doc.word_wrap,
            Command::ScrollEditor(amount, page) => self.editor()?.scroll_editor(amount, page),
            Command::SaveAll => {
                let active = self.active_tab;
                let result = (|| -> Result<()> {
                    for index in 0..self.tabs.len() {
                        self.switch_tab(index)?;
                        if !self.save(false)? {
                            break;
                        }
                    }
                    Ok(())
                })();
                self.switch_tab(active)?;
                result?;
            }
            Command::CloseAll => loop {
                let before = self.tabs.len();
                self.close_tab(window, cx)?;
                if before == 1 || self.tabs.len() == before {
                    break;
                }
            },
            Command::CopyPath => {
                let path = self
                    .doc
                    .path
                    .as_ref()
                    .context("Save this document before copying its path")?;
                cx.write_to_clipboard(ClipboardItem::new_string(
                    path.to_string_lossy().into_owned(),
                ));
            }
            Command::RevealFile => {
                let path = self
                    .doc
                    .path
                    .as_ref()
                    .context("Save this document before revealing it in Explorer")?;
                std::process::Command::new("explorer.exe")
                    .arg(format!("/select,{}", path.display()))
                    .spawn()?;
            }
            Command::PasteHtml => {
                ensure!(
                    self.doc.rich && self.doc.markdown_source.is_none(),
                    "Paste Formatted HTML requires a rich-text document."
                );
                self.editor()?.paste_html()?;
                self.doc.dirty = true;
                self.doc.queue_buffer()?;
            }
            Command::PasteOrPreview => {
                if self.doc.markdown_source.is_some()
                    || self.doc.path.as_deref().is_some_and(is_markdown)
                {
                    self.try_command(Command::ToggleMarkdown, window, cx)?;
                } else {
                    self.editor()?.execute(EditCommand::PastePlain)?;
                }
            }
            Command::QuickOpen => {
                let mut entries = Vec::new();
                let mut open_paths = Vec::new();
                for (index, tab) in self.tabs.iter().enumerate() {
                    let doc = tab.as_ref().unwrap_or(&self.doc);
                    let label = doc.path.as_ref().map_or_else(
                        || doc.untitled_name.clone(),
                        |path| path.to_string_lossy().into_owned(),
                    );
                    if let Some(path) = &doc.path {
                        open_paths.push(path);
                    }
                    entries.push(MenuEntry::item(&label, "", Command::SwitchTab(index)));
                }
                entries.extend(
                    self.prefs
                        .recent_files
                        .iter()
                        .filter(|path| !open_paths.contains(path))
                        .map(|path| {
                            MenuEntry::item(
                                &path.to_string_lossy(),
                                "",
                                Command::OpenPath(path.clone()),
                            )
                        }),
                );
                crate::palette::open_files(entries, self.queue.clone(), self.colors(window), cx);
            }
            Command::Palette => {
                let mut entries = Vec::new();
                for menu in [
                    MenuKind::App,
                    MenuKind::File,
                    MenuKind::Edit,
                    MenuKind::Find,
                    MenuKind::Font,
                    MenuKind::Format,
                    MenuKind::View,
                    MenuKind::Window,
                ] {
                    entries.extend(self.menu_entries(menu, cx)?.into_iter().filter(|entry| {
                        entry.enabled
                            && entry
                                .command
                                .as_ref()
                                .is_some_and(|c| !matches!(c, Command::Menu(_)))
                    }));
                }
                for (label, shortcut, action) in [
                    (
                        "Move Line Up",
                        crate::keyboard::label("move-line-up"),
                        LineAction::MoveUp,
                    ),
                    (
                        "Move Line Down",
                        crate::keyboard::label("move-line-down"),
                        LineAction::MoveDown,
                    ),
                    (
                        "Copy Line Up",
                        crate::keyboard::label("copy-line-up"),
                        LineAction::CopyUp,
                    ),
                    (
                        "Copy Line Down",
                        crate::keyboard::label("copy-line-down"),
                        LineAction::CopyDown,
                    ),
                    (
                        "Delete Line",
                        crate::keyboard::label("delete-line"),
                        LineAction::Delete,
                    ),
                    (
                        "Insert Line Above",
                        crate::keyboard::label("insert-line-above"),
                        LineAction::InsertAbove,
                    ),
                    (
                        "Insert Line Below",
                        crate::keyboard::label("insert-line-below"),
                        LineAction::InsertBelow,
                    ),
                    (
                        "Indent Line",
                        crate::keyboard::label("indent-line"),
                        LineAction::Indent,
                    ),
                    (
                        "Outdent Line",
                        crate::keyboard::label("outdent-line"),
                        LineAction::Outdent,
                    ),
                    (
                        "Toggle Line Comment",
                        crate::keyboard::label("toggle-comment"),
                        LineAction::ToggleComment,
                    ),
                    (
                        "Trim Trailing Whitespace",
                        crate::keyboard::label("trim-whitespace"),
                        LineAction::TrimTrailingWhitespace,
                    ),
                ] {
                    entries.push(MenuEntry::item(label, shortcut, Command::Line(action)));
                }
                entries.push(MenuEntry::item(
                    "Toggle Block Comment",
                    crate::keyboard::label("block-comment"),
                    Command::BlockComment,
                ));
                entries.push(MenuEntry::item(
                    "Toggle Full Screen",
                    crate::keyboard::label("fullscreen"),
                    Command::Fullscreen,
                ));
                entries.push(MenuEntry::item(
                    "Keyboard Shortcuts",
                    crate::keyboard::label("keyboard-help"),
                    Command::KeyboardHelp,
                ));
                entries.push(MenuEntry::item(
                    "Toggle Word Wrap",
                    crate::keyboard::label("word-wrap"),
                    Command::ToggleWordWrap,
                ));
                crate::palette::open(entries, self.queue.clone(), self.colors(window), cx);
            }
            Command::KeyboardHelp => {
                crate::palette::open_shortcuts(
                    shortcut_entries(),
                    self.queue.clone(),
                    self.colors(window),
                    cx,
                );
            }
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
            Command::MoveTab(left) => self.reorder_tab(left),
            Command::ToggleTabs => self.show_tabs = !self.show_tabs,
            Command::MoveTabs { target, scope } => self.move_tabs(target, scope, window, cx)?,
            Command::CloseWindow => {
                if self.confirm_close(window, cx) {
                    window.remove_window();
                }
            }
            Command::Open => {
                if let Some(paths) = FileDialog::new()
                    .set_title("Open")
                    .add_filter("All Files", &["*"])
                    .add_filter(
                        "Documents",
                        &[
                            "txt", "rtf", "rtfd", "html", "htm", "docx", "odt", "md", "markdown",
                        ],
                    )
                    .add_filter("Source Code", SOURCE_EXTENSIONS)
                    .pick_files()
                {
                    for path in paths {
                        self.open_file_tab(&path, window, cx)?;
                    }
                }
            }
            Command::OpenBundle => {
                if let Some(path) = FileDialog::new()
                    .set_title("Open Rich Text with Attachments")
                    .pick_folder()
                {
                    self.open_file_tab(&path, window, cx)?;
                }
            }
            Command::OpenPath(path) => self.open_file_tab(&path, window, cx)?,
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
                            if let Some(editor) = &view.doc.editor {
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
                    self.show_markdown_source()?;
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
                if prepare_instance_exit(cx) {
                    cx.quit();
                }
            }
            Command::Print => {
                let document = std::sync::Arc::new(self.editor()?.snapshot());
                let page = self.doc.page.clone();
                let queue = self.queue.clone();
                cx.spawn(async move |_, _| {
                    let command = match smol::unblock(crate::editor::printing::printers).await {
                        Ok(printers) if !printers.is_empty() => Command::ChoosePrinter {
                            printers: printers.into_iter().map(|printer| (printer.name, printer.is_default)).collect(),
                            document, page,
                        },
                        Ok(_) => Command::ReportError("No printers are available. Add a printer in system settings, or export the document as PDF.".into()),
                        Err(error) => Command::ReportError(format!("Finding printers: {error:#}")),
                    };
                    queue.borrow_mut().push_back(command);
                }).detach();
            }
            Command::ChoosePrinter {
                printers,
                document,
                page,
            } => {
                let entries = printers
                    .into_iter()
                    .map(|(printer, is_default)| {
                        MenuEntry::item(
                            &printer,
                            if is_default { "Default" } else { "" },
                            Command::PrintTo {
                                printer: printer.clone(),
                                document: document.clone(),
                                page: page.clone(),
                            },
                        )
                        .checked(is_default)
                    })
                    .collect();
                self.show_menu(entries, window, cx);
            }
            Command::PrintTo {
                printer,
                document,
                page,
            } => self.show_form(
                FormKind::Print {
                    printer: printer.clone(),
                    document,
                    page,
                },
                &format!("Print — {printer}"),
                vec![
                    Field::text("Copies", 1),
                    Field::text("Pages (blank for all)", ""),
                    Field::toggle("Print on both sides", false),
                    Field::toggle("Grayscale", false),
                ],
                window,
                cx,
            ),
            Command::ReportError(message) => bail!("{message}"),
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
            Command::GoToSelection => {
                let editor = self.editor()?;
                editor.scroll_to_selection();
                editor.focus();
            }
            Command::GoToLine => self.show_form(
                FormKind::GoToLine,
                "Go to Line",
                vec![Field::text("Line number", 1)],
                window,
                cx,
            ),
            Command::ToggleFormat => {
                self.show_markdown_source()?;
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
                self.doc.look = None;
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
                if self.doc.markdown_source.is_none() {
                    self.editor()?.set_read_only(self.doc.read_only);
                }
            }
            Command::ToggleDarkDocuments => {
                self.prefs.dark_documents = !self.prefs.dark_documents;
                self.prefs.save()?;
            }
            Command::CodeTheme(name) => {
                self.preview_theme = None;
                self.prefs.code_theme = name;
                self.prefs.save()?;
            }
            Command::PreviewCodeTheme(name) => self.preview_theme = Some(name),
            Command::EndPreview => self.preview_theme = None,
            Command::ToggleMarkdown => {
                if self.doc.markdown_source.is_some() {
                    self.show_markdown_source()?;
                } else {
                    let source = self.editor()?.text()?;
                    self.show_markdown(source)?;
                }
            }
            Command::Zoom(delta) => {
                self.doc.zoom = if delta == 0 {
                    100
                } else {
                    (i32::from(self.doc.zoom) + i32::from(delta)).clamp(25, 400) as u16
                };
                self.editor()?.set_zoom(self.doc.zoom);
            }
            Command::FontStep(delta) => {
                let editor = self.editor()?;
                let size = editor.formatting().size;
                let size = if size > 0. { size } else { 12. };
                editor.execute(EditCommand::FontSize((size + delta).clamp(1., 1638.)))?;
            }
            Command::InterfaceScale(percent) => {
                ensure!(
                    ui::SCALE_RANGE.contains(&percent),
                    "Interface size must be between {}% and {}%",
                    ui::SCALE_RANGE.start(),
                    ui::SCALE_RANGE.end()
                );
                ui::set_scale_percent(percent);
                self.prefs.interface_scale = percent;
                self.prefs.save()?;
                cx.refresh_windows();
                window.refresh();
            }
            Command::CustomInterfaceScale => self.show_form(
                FormKind::InterfaceScale,
                "Interface Size",
                vec![Field::text("Size (%)", ui::scale_percent())],
                window,
                cx,
            ),
            Command::CustomFontSize => self.show_form(
                FormKind::FontSize,
                "Font Size",
                vec![Field::text("Size (points)", self.doc.formatting.size)],
                window,
                cx,
            ),
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
                self.doc.queue_buffer()?;
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
                self.show_markdown_source()?;
                let text = self.editor()?.text()?;
                self.doc.proofing.manual = true;
                self.update_proofing(Some(&text), window, cx)?;
            }
            Command::ExplainProofing(message) => {
                MessageDialog::new()
                    .set_title("Spelling and Grammar")
                    .set_description(message)
                    .show();
            }
            Command::LanguageTools => crate::language_tools::open(
                self.prefs.proofing.clone(),
                self.queue.clone(),
                Palette::new(self.is_dark(window)),
                cx,
            ),
            Command::ApplyLanguageTools(options) => {
                options.validate()?;
                self.prefs.proofing = options;
                self.prefs.save()?;
                self.doc.proofing.invalidate();
                self.editor()?.set_proofing_marks(&[])?;
            }
            Command::ProofingCorrection {
                document,
                source,
                range,
                original,
                replacement,
            } => {
                ensure!(
                    document == self.doc.recovery_id && self.doc.markdown_source.is_none(),
                    "Return to the checked document and check it again before applying this suggestion."
                );
                let editor = self.editor()?;
                ensure!(
                    crate::language_tools::correction_matches(
                        &editor.text()?,
                        &source,
                        &range,
                        &original
                    ),
                    "The document changed. Check spelling and grammar again before applying this suggestion."
                );
                editor.select_range(range)?;
                editor.replace_current(&replacement)?;
            }
            Command::Complete => {
                if let Some((_, surface)) = &self.surface {
                    surface.update(cx, |surface, cx| surface.complete(window, cx));
                }
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
                        self.editor()?.set_proofing_marks(&[])?;
                        self.doc.proofing.invalidate();
                    }
                    _ => {}
                }
                self.prefs.save()?;
            }
            Command::Edit(edit) => {
                ensure!(
                    self.doc.markdown_source.is_none()
                        || matches!(
                            edit,
                            EditCommand::Copy | EditCommand::SelectAll | EditCommand::CopyStyle
                        ),
                    "This is the formatted view of a Markdown file. Choose View > Show Markdown Source to edit it."
                );
                if self.doc.read_only
                    && !matches!(
                        edit,
                        EditCommand::Copy | EditCommand::SelectAll | EditCommand::CopyStyle
                    )
                {
                    bail!("This document is locked. Unlock it in the Format menu to make changes.");
                }
                // Plain text has one style for the whole document, so TextEdit disables
                // character and paragraph styling there; only font and size apply.
                if !self.doc.rich
                    && matches!(
                        edit,
                        EditCommand::Bold
                            | EditCommand::Italic
                            | EditCommand::Underline
                            | EditCommand::Strikethrough
                            | EditCommand::Superscript
                            | EditCommand::Subscript
                            | EditCommand::Foreground(_)
                            | EditCommand::Background(_)
                            | EditCommand::Align(_)
                            | EditCommand::LineSpacing(_)
                            | EditCommand::Indent(_)
                            | EditCommand::FirstLineIndent(_)
                            | EditCommand::TabStop(_)
                            | EditCommand::List(_)
                            | EditCommand::PasteStyle
                    )
                {
                    return Ok(());
                }
                let editor = self.editor()?;
                let selection = editor.selection();
                if selection.is_empty() && matches!(edit, EditCommand::Copy | EditCommand::Cut) {
                    editor.select_range(crate::editing::select_line(
                        &editor.text()?,
                        selection.clone(),
                    )?)?;
                    let copied = matches!(edit, EditCommand::Copy);
                    editor.execute(edit)?;
                    if copied {
                        editor.select_range(selection)?;
                    }
                } else {
                    editor.execute(edit)?;
                }
            }
            Command::Menu(kind) => {
                let entries = self.menu_entries(kind, cx)?;
                self.show_menu(entries, window, cx);
            }
            Command::Submit(kind, values) => self.submit(kind, values, window, cx)?,
            Command::About => {
                MessageDialog::new().set_title("About ExEditor").set_description("ExEditor 0.1.0\nA editor document editor built with Rust and GPUI Kit.\nhttps://github.com/abelokoj/ExEditor\n\nTextEdit is a trademark of Apple Inc. ExEditor is an independent application.").show();
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
            let n = value(index)?
                .trim()
                .parse::<f32>()
                .context("Enter a number")?;
            ensure!(
                n.is_finite() && n > 0. && n <= 1000.,
                "Enter a number greater than zero and at most 1000"
            );
            Ok(n)
        };
        match kind {
            FormKind::Print {
                printer,
                document,
                page,
            } => {
                let copies = value(0)?
                    .trim()
                    .parse::<u32>()
                    .context("Enter a whole number of copies")?;
                ensure!(
                    (1..=999).contains(&copies),
                    "Copies must be between 1 and 999."
                );
                let range = value(1)?.trim();
                let options = crate::editor::printing::PrintOptions {
                    printer,
                    copies,
                    page_range: (!range.is_empty()).then(|| range.to_owned()),
                    duplex: value(2)? == "true",
                    grayscale: value(3)? == "true",
                };
                let queue = self.queue.clone();
                cx.spawn(async move |_, _| {
                    if let Err(error) = smol::unblock(move || {
                        crate::editor::printing::submit(&document, &page, &options)
                    })
                    .await
                    {
                        queue.borrow_mut().push_back(Command::ReportError(format!(
                            "Printing the document: {error:#}"
                        )));
                    }
                })
                .detach();
            }
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
            FormKind::InterfaceScale => {
                let percent = value(0)?
                    .trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse::<u16>()
                    .context("Enter a whole percentage, such as 125")?;
                self.try_command(Command::InterfaceScale(percent), window, cx)?;
            }
            FormKind::FontSize => self.editor()?.execute(EditCommand::FontSize(number(0)?))?,
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
                self.editor()?.set_page_settings(&page)?;
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
                if let Some(prefix) = values.get(14) {
                    crate::names::validate_prefix(prefix)?;
                    prefs.untitled_prefix = prefix.trim().to_owned();
                }
                if let Some(autocomplete) = values.get(15) {
                    prefs.autocomplete = autocomplete == "true";
                }
                prefs.save()?;
                self.prefs = prefs;
                self.doc.look = None;
                // Show a new default font now where it can't overwrite chosen styling:
                // plain text has one font, and an empty rich document has none yet.
                let editor = self.editor()?;
                if self.doc.markdown_source.is_none()
                    && (!self.doc.rich || self.doc.last_text.is_empty())
                {
                    let (font, size) = if self.doc.rich {
                        (self.prefs.rich_font.clone(), self.prefs.rich_size)
                    } else {
                        (self.prefs.plain_font.clone(), self.prefs.plain_size)
                    };
                    let dirty = self.doc.dirty || editor.is_modified();
                    editor.execute(EditCommand::Font(font))?;
                    editor.execute(EditCommand::FontSize(size))?;
                    if !dirty {
                        editor.mark_saved();
                    }
                }
                self.editor()?.set_auto_links(self.prefs.automatic_links);
                self.editor()?.set_proofing_marks(&[])?;
                self.doc.proofing.invalidate();
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
    fn menu_entries(&self, kind: MenuKind, cx: &Context<Self>) -> Result<Vec<MenuEntry>> {
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
                item(
                    "Settings…",
                    crate::keyboard::label("preferences"),
                    C::Preferences,
                ),
                sep(),
                item("Quit ExEditor", crate::keyboard::label("quit"), C::Quit),
            ],
            M::File => vec![
                item("New Window", crate::keyboard::label("new"), C::New),
                item("New Tab", crate::keyboard::label("new-tab"), C::NewTab),
                item(
                    "Reopen Closed File",
                    crate::keyboard::label("reopen-closed"),
                    C::ReopenClosed,
                ),
                item("Save All", crate::keyboard::label("save-all"), C::SaveAll),
                item("Open…", crate::keyboard::label("open"), C::Open),
                item(
                    "Quick Open…",
                    crate::keyboard::label("quick-open"),
                    C::QuickOpen,
                ),
                item("Open RTFD Folder…", "", C::OpenBundle),
                item(
                    "Open Recent",
                    crate::keyboard::label("recent-files"),
                    C::Menu(M::Recent),
                ),
                sep(),
                item("Close", crate::keyboard::label("close"), C::Close),
                item("Save…", crate::keyboard::label("save"), C::Save),
                item("Save As…", crate::keyboard::label("save-as"), C::SaveAs),
                item("Duplicate", "", C::Duplicate),
                item("Rename…", "", C::Rename),
                item("Move To…", "", C::Move),
                item("Revert to Saved", "", C::Revert),
                item("Browse All Versions…", "", C::Menu(M::Versions)),
                sep(),
                item("Export as PDF…", "", C::Pdf),
                item("Show Properties", "", C::Properties),
                sep(),
                item(
                    "Page Setup…",
                    crate::keyboard::label("page-setup"),
                    C::PageSetup,
                ),
                item("Print…", crate::keyboard::label("print"), C::Print),
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
                e("Undo", crate::keyboard::label("undo"), EditCommand::Undo),
                e("Redo", crate::keyboard::label("redo"), EditCommand::Redo),
                sep(),
                e("Cut", crate::keyboard::label("cut"), EditCommand::Cut),
                e("Copy", crate::keyboard::label("copy"), EditCommand::Copy),
                e("Paste", crate::keyboard::label("paste"), EditCommand::Paste),
                {
                    let mut entry = item("Paste Formatted HTML", "", C::PasteHtml);
                    entry.enabled =
                        self.doc.rich && !self.doc.read_only && self.doc.markdown_source.is_none();
                    entry
                },
                e("Paste and Match Style", "", EditCommand::PastePlain),
                e("Delete", "", EditCommand::Delete),
                e(
                    "Select All",
                    crate::keyboard::label("select-all"),
                    EditCommand::SelectAll,
                ),
                sep(),
                item("Find", "›", C::Menu(M::Find)),
                item("Spelling and Grammar", "›", C::Menu(M::Spelling)),
                item("Substitutions", "›", C::Menu(M::Substitutions)),
                item("Transformations", "›", C::Menu(M::Transformations)),
                sep(),
                item("Start Speaking", "", C::Speak),
                item("Stop Speaking", "", C::StopSpeaking),
                item("Complete", crate::keyboard::label("complete"), C::Complete),
            ],
            M::Find => vec![
                item("Find…", crate::keyboard::label("find"), C::Find),
                item(
                    "Find and Replace…",
                    crate::keyboard::label("replace"),
                    C::Replace,
                ),
                item(
                    "Find Next",
                    crate::keyboard::label("find-next"),
                    C::FindNext,
                ),
                item(
                    "Find Previous",
                    crate::keyboard::label("find-previous"),
                    C::FindPrevious,
                ),
                item(
                    "Use Selection for Find",
                    crate::keyboard::label("use-selection"),
                    C::UseSelection,
                ),
                item(
                    "Jump to Selection",
                    crate::keyboard::label("jump-to-selection"),
                    C::GoToSelection,
                ),
                item(
                    "Go to Line…",
                    crate::keyboard::label("go-to-line"),
                    C::GoToLine,
                ),
            ],
            M::Spelling => vec![
                item(
                    "Show Spelling and Grammar…",
                    crate::keyboard::label("show-spelling"),
                    C::CheckSpelling,
                ),
                item(
                    "Check Document Now",
                    crate::keyboard::label("check-spelling"),
                    C::CheckSpelling,
                ),
                sep(),
                item("Check While Typing", "", C::Toggle("spelling"))
                    .checked(self.prefs.spellcheck),
                item("Language Tools…", "", C::LanguageTools),
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
                    crate::keyboard::label("toggle-format"),
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
                item("Show Fonts", crate::keyboard::label("font"), C::Font),
                item("Bigger", crate::keyboard::label("bigger"), C::FontStep(1.)),
                item(
                    "Smaller",
                    crate::keyboard::label("smaller"),
                    C::FontStep(-1.),
                ),
                item("Other Size…", "", C::CustomFontSize),
                e("Bold", crate::keyboard::label("bold"), EditCommand::Bold)
                    .checked(self.doc.formatting.bold),
                e(
                    "Italic",
                    crate::keyboard::label("italic"),
                    EditCommand::Italic,
                )
                .checked(self.doc.formatting.italic),
                e(
                    "Underline",
                    crate::keyboard::label("underline"),
                    EditCommand::Underline,
                )
                .checked(self.doc.formatting.underline),
                e("Strikethrough", "", EditCommand::Strikethrough)
                    .checked(self.doc.formatting.strikethrough),
                sep(),
                e("Superscript", "", EditCommand::Superscript),
                e("Subscript", "", EditCommand::Subscript),
                sep(),
                // shortcut: no keys; TextEdit's Option-Cmd-C/V would be AltGr typing on many layouts.
                e("Copy Style", "", EditCommand::CopyStyle),
                e("Paste Style", "", EditCommand::PasteStyle),
                sep(),
                item(
                    "Show Colors…",
                    crate::keyboard::label("colors"),
                    C::Foreground,
                ),
                item("Text Background Color…", "", C::Background),
                item("Insert Color Code…", "", C::InsertColorCode),
            ],
            M::Alignment => [
                ("Left", "", Alignment::Left),
                ("Center", "", Alignment::Center),
                ("Right", "", Alignment::Right),
                ("Justified", "", Alignment::Justify),
            ]
            .into_iter()
            .map(|(name, shortcut, alignment)| {
                e(name, shortcut, EditCommand::Align(alignment))
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
                    crate::keyboard::label("ruler"),
                    C::ToggleRuler,
                ),
                sep(),
                item("Dark Document Background", "", C::ToggleDarkDocuments)
                    .checked(self.prefs.dark_documents),
                item("Code Theme", "›", C::Menu(M::CodeTheme)),
            ]
            .into_iter()
            .chain(
                (self.doc.markdown_source.is_some()
                    || (!self.doc.rich && self.doc.path.as_deref().is_some_and(is_markdown)))
                .then(|| {
                    item(
                        if self.doc.markdown_source.is_some() {
                            "Show Markdown Source"
                        } else {
                            "Show Formatted Markdown"
                        },
                        "",
                        C::ToggleMarkdown,
                    )
                }),
            )
            .chain([
                sep(),
                item(
                    "Actual Size",
                    crate::keyboard::label("actual-size"),
                    C::Zoom(0),
                ),
                item("Zoom In", crate::keyboard::label("zoom-in"), C::Zoom(10)),
                item("Zoom Out", crate::keyboard::label("zoom-out"), C::Zoom(-10)),
                sep(),
                item("Interface Size", "›", C::Menu(M::InterfaceSize)),
            ])
            .collect(),
            M::CodeTheme => std::iter::once(syntax::AUTOMATIC)
                .chain(syntax::THEMES.iter().map(|&(name, _)| name))
                .map(|name| {
                    item(name, "", C::CodeTheme(name.into()))
                        .checked(self.prefs.code_theme == name)
                        .previewing(C::PreviewCodeTheme(name.into()))
                })
                .collect(),
            M::InterfaceSize => [75, 90, 100, 110, 125, 150, 175, 200]
                .into_iter()
                .map(|percent| {
                    item(&format!("{percent}%"), "", C::InterfaceScale(percent))
                        .checked(ui::scale_percent() == percent)
                })
                .chain([sep(), item("Custom…", "", C::CustomInterfaceScale)])
                .collect(),
            M::Window => vec![
                item(
                    "Next Tab",
                    crate::keyboard::label("next-tab"),
                    C::NextTab(false),
                ),
                item(
                    "Previous Tab",
                    crate::keyboard::label("previous-tab"),
                    C::NextTab(true),
                ),
                item(
                    "Move Tab Left",
                    crate::keyboard::label("move-tab-left"),
                    C::MoveTab(true),
                ),
                item(
                    "Move Tab Right",
                    crate::keyboard::label("move-tab-right"),
                    C::MoveTab(false),
                ),
                sep(),
                item("New Window", crate::keyboard::label("new"), C::New),
                item(
                    "Move Tab to Window",
                    "›",
                    C::Menu(M::MoveTabs(TransferScope::CurrentTab)),
                ),
                item(
                    "Move All Tabs to Window",
                    "›",
                    C::Menu(M::MoveTabs(TransferScope::AllTabs)),
                ),
                sep(),
                item(&self.name(), "", C::GoToSelection),
            ],
            M::MoveTabs(scope) => {
                let own = cx.entity().entity_id();
                let mut entries: Vec<_> = cx
                    .global::<OpenEditors>()
                    .0
                    .iter()
                    .enumerate()
                    .filter_map(|(index, (_, editor))| {
                        if editor.entity_id() == own {
                            return None;
                        }
                        let target = editor.upgrade()?;
                        let view = target.read(cx);
                        Some(item(
                            &format!("{} — Window {}", view.name(), index + 1),
                            "",
                            C::MoveTabs {
                                target: editor.entity_id(),
                                scope,
                            },
                        ))
                    })
                    .collect();
                if entries.is_empty() {
                    let mut entry = item("No other document windows", "", C::GoToSelection);
                    entry.enabled = false;
                    entries.push(entry);
                }
                entries
            }
            M::Help => vec![
                item(
                    "Command Palette…",
                    crate::keyboard::label("command-palette"),
                    C::Palette,
                ),
                item(
                    "Keyboard Shortcuts…",
                    crate::keyboard::label("keyboard-help"),
                    C::KeyboardHelp,
                ),
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
            .chain([sep(), item("Other…", "", C::CustomFontSize)])
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
                    if size.fract() == 0. {
                        format!("{size:.0} ⌄")
                    } else {
                        format!("{size:.1} ⌄")
                    },
                    48.,
                    false,
                    Command::Menu(MenuKind::Sizes),
                    c,
                    cx,
                )
                .into_any_element(),
                ui::control("text-color", "", 24., false, c)
                    .tooltip("Text Color")
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
                // TextEdit shows text and background colors as two color wells.
                ui::control("text-background", "", 24., false, c)
                    .tooltip("Text Background Color")
                    .child(
                        div()
                            .size(px(12.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(rgb(c.border))
                            .bg(rgb(self.doc.formatting.background)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.command(Command::Background, window, cx)
                    }))
                    .into_any_element(),
            ];
        let styles: [(&'static str, &'static str, bool, EditCommand); 4] = [
            ("bold", "Bold", self.doc.formatting.bold, EditCommand::Bold),
            (
                "italic",
                "Italic",
                self.doc.formatting.italic,
                EditCommand::Italic,
            ),
            (
                "underline",
                "Underline",
                self.doc.formatting.underline,
                EditCommand::Underline,
            ),
            (
                "strikethrough",
                "Strikethrough",
                self.doc.formatting.strikethrough,
                EditCommand::Strikethrough,
            ),
        ];
        for (id, name, active, edit) in styles {
            // Each button draws its own effect, like TextEdit's B I U S segments.
            let glyph = div().child(&name[..1]);
            let glyph = match id {
                "bold" => glyph.font_weight(FontWeight::BOLD),
                "italic" => glyph.italic(),
                "underline" => glyph.underline(),
                _ => glyph.line_through(),
            };
            let command = Command::Edit(edit);
            controls.push(
                ui::control(id, "", 22., active, c)
                    .tooltip(name)
                    .child(glyph)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.command(command.clone(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        for (id, name, data, alignment) in [
            ("left", "Align Left", ui::ALIGN_LEFT, Alignment::Left),
            ("center", "Center", ui::ALIGN_CENTER, Alignment::Center),
            ("right", "Align Right", ui::ALIGN_RIGHT, Alignment::Right),
            ("justify", "Justify", ui::ALIGN_JUSTIFY, Alignment::Justify),
        ] {
            let command = Command::Edit(EditCommand::Align(alignment));
            let active = self.doc.formatting.alignment == alignment;
            controls.push(
                ui::control(id, "", 22., active, c)
                    .tooltip(name)
                    .child(ui::icon(data, if active { 0xffffff } else { c.ink }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.command(command.clone(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        controls.push(
            ui::control("line-spacing", "", 32., false, c)
                .tooltip("Line and Paragraph Spacing")
                .child(ui::icon(ui::LINE_SPACING, c.ink))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.command(Command::Menu(MenuKind::Spacing), window, cx)
                }))
                .into_any_element(),
        );
        controls.push(
            ui::control("list", "", 32., false, c)
                .tooltip("Lists")
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
#[derive(Default)]
struct RecentlyClosed(Vec<PathBuf>);
impl Global for RecentlyClosed {}
impl Global for OpenEditors {}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-kit components size text in rem; scale it with View > Interface Size.
        window.set_rem_size(ui::rem_size());
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
                    // Keeps the title bar's drag area from swallowing clicks on the button.
                    .occlude()
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
            // GPUI's start_window_move is a no-op on Windows; a drag area is answered as
            // HTCAPTION, so Windows moves the window and double-click zooms it.
            .window_control_area(WindowControlArea::Drag)
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
            .when_some(self.focus.as_ref(), |root, focus| root.track_focus(focus))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match this
                    .chord
                    .resolve_keystroke(&event.keystroke, Instant::now())
                {
                    crate::keyboard::Outcome::Ignored => return,
                    crate::keyboard::Outcome::Action(action) => {
                        if let Some(command) = shortcut_command(action) {
                            this.command(command, window, cx);
                        }
                    }
                    crate::keyboard::Outcome::Prefix | crate::keyboard::Outcome::Cancelled => {}
                }
                cx.stop_propagation();
                window.prevent_default();
            }))
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
                    .child(
                        // Tabs keep their width and scroll sideways (wheel or trackpad)
                        // once they no longer fit; "+" stays in reach at the end.
                        div()
                            .id("document-tab-strip")
                            .flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                                let name = tab
                                    .as_ref()
                                    .map(DocumentTab::name)
                                    .unwrap_or_else(|| self.name());
                                ui::control(("tab", index), name, 160., index == current, c)
                                    .flex_shrink_0()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.command(Command::SwitchTab(index), window, cx)
                                    }))
                            })),
                    )
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
            let editor = self.doc.editor.clone();
            root = root.child(ui::ruler(c, self.doc.zoom, move |position, _, _| {
                if let Some(editor) = &editor {
                    let _ = editor.execute(EditCommand::TabStop(
                        ((position - 12.).max(0.) * 15.).round() as i32,
                    ));
                }
            }));
        }
        if let Some(editor) = self.doc.editor.clone() {
            editor.set_wrap(self.doc.word_wrap);
            editor.set_wrap_to_page(self.doc.wrap_page);
            if self
                .surface
                .as_ref()
                .is_none_or(|(current, _)| !Rc::ptr_eq(current, &editor))
            {
                let surface = cx.new(|cx| EditorSurface::new(editor.clone(), cx));
                self.surface = Some((editor.clone(), surface));
            }
            if let Some((_, surface)) = &self.surface {
                root = root.child(
                    div()
                        .id("document")
                        .flex_1()
                        .min_h(px(1.))
                        .w_full()
                        .bg(rgb(if self.doc.wrap_page {
                            c.chrome
                        } else {
                            self.doc.paper
                        }))
                        .child(surface.clone()),
                );
            }
        } else {
            root = root.child(div().p(px(24.)).child(
                "The document editor could not be initialized. Close this window and try again.",
            ));
        }
        root
    }
}

impl DocumentTab {
    fn new(prefs: &Preferences) -> Self {
        let editor = match Editor::new() {
            Ok(editor) => Some(Rc::new(editor)),
            Err(error) => {
                show_error(&error);
                None
            }
        };
        if let Some(editor) = &editor {
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
            editor,
            recovery_id,
            buffer_lease: buffer_lease.map(std::sync::Arc::new),
            untitled_name: crate::names::next_name(&prefs.untitled_prefix),
            retain_buffer: true,
            ..Self::vacant(prefs)
        }
    }
    // An emptied transfer source is removed before rendering; this owns no editor control or buffer.
    fn vacant(prefs: &Preferences) -> Self {
        Self {
            editor: None,
            path: None,
            format: if prefs.rich_text {
                DocumentFormat::Rtf
            } else {
                DocumentFormat::PlainText
            },
            encoding: prefs.default_encoding,
            line_ending: LineEnding::CrLf,
            fingerprint: None,
            observed_fingerprint: None,
            rich: prefs.rich_text,
            dirty: false,
            read_only: false,
            ruler: prefs.show_ruler,
            wrap_page: prefs.wrap_to_page,
            word_wrap: true,
            untitled_name: String::new(),
            zoom: 100,
            formatting: FormatState::default(),
            page: PageSettings::default(),
            recovery_id: String::new(),
            buffer_lease: None,
            pending_buffer: None,
            buffered: false,
            retain_buffer: false,
            buffer_error_reported: false,
            restored_buffer: false,
            last_recovery: Instant::now(),
            last_external_check: Instant::now(),
            last_text: String::new(),
            find_text: String::new(),
            replace_text: String::new(),
            find_case: false,
            find_whole: false,
            highlighter: None,
            look: None,
            paper: 0xffffff,
            markdown_source: None,
            proofing: crate::language_tools::CheckState::default(),
        }
    }

    fn restore(prefs: &Preferences, buffer: RestoredBuffer) -> Result<Self> {
        let mut tab = Self::new(prefs);
        let editor = tab
            .editor
            .as_ref()
            .context("The document editor could not be created")?;
        editor.set_plain_text_mode(buffer.document.format.is_plain())?;
        if let Some(snapshot) = buffer.document.editor_snapshot {
            editor.set_document(snapshot)?;
        } else {
            match &buffer.document.content {
                DocumentContent::Plain(text) => editor.set_text(text)?,
                DocumentContent::Rich(bytes) => editor.set_rtf(bytes)?,
            }
        }
        tab.page = editor.page_settings();
        if buffer.document.format.is_plain() {
            editor.execute(EditCommand::Font(prefs.plain_font.clone()))?;
            editor.execute(EditCommand::FontSize(prefs.plain_size))?;
        }
        tab.last_text = editor.text()?;
        tab.formatting = editor.formatting();
        editor.acknowledge_buffered_changes();
        if let Some(name) = buffer.document.untitled_name {
            crate::names::reserve_name(&name);
            tab.untitled_name = name;
        }
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
        tab.buffer_lease = Some(std::sync::Arc::new(buffer.lease));
        tab.dirty = true;
        tab.restored_buffer = true;
        tab.buffered = true;
        Ok(tab)
    }
    fn canonical_editor(&self) -> Option<&Rc<Editor>> {
        self.markdown_source.as_ref().or(self.editor.as_ref())
    }
    fn snapshot(&mut self) -> Result<Option<RecoveryDocument>> {
        if !self.retain_buffer {
            return Ok(None);
        }
        let Some(editor) = self.canonical_editor().cloned() else {
            return Ok(None);
        };
        self.dirty |= editor.is_modified();
        if !self.dirty {
            return Ok(None);
        }
        if self.buffer_lease.is_none() {
            self.buffer_lease = session::lease_buffer(&self.recovery_id)?.map(std::sync::Arc::new);
        }
        ensure!(
            self.buffer_lease.is_some(),
            "Another ExEditor instance owns this unsaved buffer; save a copy before closing."
        );
        let content = if self.markdown_source.is_some() || !self.rich {
            DocumentContent::Plain(editor.text()?)
        } else {
            DocumentContent::Rich(editor.rtf()?)
        };
        Ok(Some(RecoveryDocument {
            id: self.recovery_id.clone(),
            untitled_name: Some(self.untitled_name.clone()),
            original_path: self.path.clone(),
            format: self.format,
            content,
            editor_snapshot: Some(editor.snapshot()),
            encoding: self.encoding,
            line_ending: self.line_ending,
            saved_at: chrono::Local::now().to_rfc3339(),
        }))
    }
    fn poll_buffer(&mut self) -> Result<()> {
        use std::sync::mpsc::TryRecvError;
        let result = match self
            .pending_buffer
            .as_ref()
            .map(|receiver| receiver.try_recv())
        {
            None | Some(Err(TryRecvError::Empty)) => return Ok(()),
            Some(Err(TryRecvError::Disconnected)) => Err(anyhow::anyhow!(
                "The recovery writer stopped before confirming the snapshot"
            )),
            Some(Ok(result)) => result,
        };
        self.pending_buffer = None;
        result?;
        self.buffered = true;
        self.buffer_error_reported = false;
        Ok(())
    }
    fn finish_buffer(&mut self) -> Result<()> {
        if let Some(receiver) = self.pending_buffer.take() {
            receiver
                .recv()
                .context("Waiting for the unsaved buffer to finish writing")??;
            self.buffered = true;
        }
        Ok(())
    }
    fn queue_buffer(&mut self) -> Result<()> {
        self.poll_buffer()?;
        if self.pending_buffer.is_some() {
            return Ok(());
        }
        if let Some(snapshot) = self.snapshot()? {
            let lease = self
                .buffer_lease
                .as_ref()
                .context("The buffer lease is missing")?
                .clone();
            self.pending_buffer = Some(session::queue_recovery(snapshot, lease));
            if let Some(editor) = self.canonical_editor() {
                editor.acknowledge_buffered_changes();
            }
            self.last_recovery = Instant::now();
        }
        Ok(())
    }
    fn discard_recovery(&mut self) -> Result<()> {
        // Waiting prevents an older queued snapshot resurrecting a saved/discarded tab.
        let _ = self.finish_buffer();
        document::remove_recovery(&self.recovery_id)
    }
    fn persist_buffer(&mut self) -> Result<()> {
        let _ = self.finish_buffer();
        if let Some(snapshot) = self.snapshot()? {
            document::write_recovery(&snapshot)?;
            if let Some(editor) = self.canonical_editor() {
                editor.acknowledge_buffered_changes();
            }
            self.buffered = true;
            self.buffer_error_reported = false;
            self.last_recovery = Instant::now();
        }
        Ok(())
    }
    fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.untitled_name.clone())
    }
}

#[cfg(test)]
mod window_tests {
    use super::{DocumentTab, EditorWindow, TransferScope};
    use crate::{preferences::Preferences, services::Speech};
    use gpui_kit::{ScrollHandle, Task};

    fn window_with_tabs(names: &[&str], active: usize) -> EditorWindow {
        let prefs = Preferences::default();
        let mut tabs: Vec<_> = names
            .iter()
            .map(|name| {
                let mut tab = DocumentTab::vacant(&prefs);
                tab.untitled_name = (*name).into();
                tab.recovery_id = (*name).into();
                tab.last_text = format!("unsaved {name} café 😀");
                tab.dirty = true;
                Some(tab)
            })
            .collect();
        let doc = tabs[active].take().expect("active document");
        EditorWindow {
            doc,
            tabs,
            active_tab: active,
            show_tabs: true,
            queue: Default::default(),
            prefs,
            speech: Speech::default(),
            was_active: false,
            preview_theme: None,
            tab_scroll: ScrollHandle::new(),
            chord: crate::keyboard::State::default(),
            focus: None,
            surface: None,
            _poll: Task::ready(()),
        }
    }
    fn names(window: &EditorWindow) -> Vec<String> {
        assert_eq!(window.tabs.iter().filter(|tab| tab.is_none()).count(), 1);
        window
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                if index == window.active_tab {
                    &window.doc
                } else {
                    tab.as_ref().expect("inactive document")
                }
                .recovery_id
                .clone()
            })
            .collect()
    }
    #[test]
    fn every_binding_reaches_an_application_command() {
        for binding in crate::keyboard::BINDINGS {
            if binding.action != "chord" {
                assert!(
                    super::shortcut_command(binding.action).is_some(),
                    "{binding:?}"
                );
            }
        }
    }
    #[test]
    fn shortcut_reference_has_every_canonical_command_and_its_current_binding() {
        let entries = super::shortcut_entries();
        for binding in crate::keyboard::BINDINGS
            .iter()
            .filter(|binding| !binding.label.is_empty() && binding.action != "chord")
        {
            let entry = entries
                .iter()
                .find(|entry| entry.shortcut == binding.label)
                .expect("canonical binding is visible");
            assert_eq!(
                format!("{:?}", entry.command),
                format!("{:?}", super::shortcut_command(binding.action))
            );
        }
    }
    #[test]
    fn reordering_tabs_keeps_live_document_and_buffer_identity() {
        let mut window = window_with_tabs(&["one", "two", "three"], 1);
        let text = window.doc.last_text.clone();
        window.reorder_tab(true);
        assert_eq!(names(&window), ["two", "one", "three"]);
        assert_eq!(window.active_tab, 0);
        window.reorder_tab(true);
        assert_eq!(names(&window), ["two", "one", "three"]);
        window.reorder_tab(false);
        window.reorder_tab(false);
        window.reorder_tab(false);
        assert_eq!(names(&window), ["one", "three", "two"]);
        assert_eq!(window.active_tab, 2);
        assert_eq!(window.doc.last_text, text);
    }
    #[test]
    fn moving_current_tab_keeps_other_documents_and_buffer_identity() {
        for active in 0..3 {
            let mut source = window_with_tabs(&["one", "two", "three"], active);
            let mut target = window_with_tabs(&["target one", "target two"], 1);
            let original = source.doc.last_text.clone();
            let id = source.doc.recovery_id.clone();
            let (moved, index) = source.take_transfer(TransferScope::CurrentTab);
            target.receive_transfer(moved, index);
            let remaining: Vec<_> = ["one", "two", "three"]
                .into_iter()
                .filter(|name| *name != id)
                .map(String::from)
                .collect();
            assert_eq!(names(&source), remaining);
            assert_eq!(
                names(&target),
                vec!["target one".to_owned(), "target two".to_owned(), id]
            );
            assert_eq!(target.doc.last_text, original);
            assert!(target.doc.dirty);
            assert_eq!(target.active_tab, 2);
        }
    }
    #[test]
    fn moving_whole_window_preserves_tab_order_and_selected_document() {
        for active in 0..3 {
            let mut source = window_with_tabs(&["one", "two", "three"], active);
            let mut target = window_with_tabs(&["target"], 0);
            let (moved, selected) = source.take_transfer(TransferScope::AllTabs);
            target.receive_transfer(moved, selected);
            assert_eq!(names(&target), ["target", "one", "two", "three"]);
            assert_eq!(target.active_tab, active + 1);
            assert!(source.doc.editor.is_none());
            assert!(!source.doc.retain_buffer);
            assert!(source.doc.recovery_id.is_empty());
        }
    }
    #[test]
    fn moving_last_tab_leaves_only_an_inert_source_placeholder() {
        let mut source = window_with_tabs(&["only"], 0);
        let (moved, selected) = source.take_transfer(TransferScope::CurrentTab);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].recovery_id, "only");
        assert_eq!(selected, 0);
        assert!(!source.doc.retain_buffer);
        assert_eq!(source.tabs.len(), 1);
        assert!(source.tabs[0].is_none());
    }
    #[test]
    fn reopening_same_canonical_file_selects_its_existing_dirty_tab() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let file = directory.path().join("notes café.md");
        std::fs::write(&file, "original on disk").expect("file");
        let mut window = window_with_tabs(&["first", "edited file"], 0);
        let edited = window.tabs[1].as_mut().expect("inactive file");
        edited.path = Some(file.clone());
        let original = edited.last_text.clone();
        assert_eq!(
            window.tab_for_path(&directory.path().join(".").join("notes café.md")),
            Some(1)
        );
        assert_eq!(
            window.tabs[1].as_ref().expect("existing buffer").last_text,
            original
        );
        assert!(window.tabs[1].as_ref().expect("existing buffer").dirty);
        assert_eq!(window.tabs.len(), 2);
    }
}
