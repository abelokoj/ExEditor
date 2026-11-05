//! Windows RichEdit host. Every HWND stays on its creating UI thread.
use anyhow::{Context, Result, bail, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt::Write as _,
    marker::PhantomData,
    mem::size_of,
    ops::Range,
    path::Path,
    ptr,
    rc::Rc,
    sync::OnceLock,
};
use windows::{
    Win32::UI::Controls::RichEdit::{
        ITextDocument, tomApplyNow, tomApplyTmp, tomAutoColor, tomUndefined,
    },
    core::{IUnknown, Interface},
};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND, POINT, RECT},
    Graphics::Gdi::{
        ClientToScreen, DeleteDC, GetDeviceCaps, GetStockObject, HDC, InvalidateRect, LOGPIXELSX,
        LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH,
        SetViewportOrgEx, WHITE_BRUSH,
    },
    Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
    System::LibraryLoader::{GetModuleHandleW, LoadLibraryW},
    UI::{
        Controls::{
            Dialogs::{
                CommDlgExtendedError, PD_PAGENUMS, PD_RETURNDC, PD_SELECTION,
                PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW,
            },
            EM_CANUNDO, EM_EMPTYUNDOBUFFER, EM_GETMODIFY, EM_REPLACESEL, EM_SCROLLCARET,
            EM_SETMARGINS, EM_SETMODIFY, EM_SETREADONLY, ShowScrollBar,
        },
        Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
            DestroyWindow, ES_AUTOVSCROLL, ES_MULTILINE, ES_NOHIDESEL, ES_WANTRETURN, GW_CHILD,
            GW_OWNER, GetCursorPos, GetWindow, IsIconic, IsWindowVisible, MF_GRAYED, MF_SEPARATOR,
            MF_STRING, MoveWindow, PostMessageW, RegisterClassW, SB_HORZ, SW_HIDE, SWP_HIDEWINDOW,
            SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW,
            SetWindowPos, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WA_ACTIVE,
            WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_CLEAR, WM_CONTEXTMENU, WM_COPY, WM_CUT,
            WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
            WM_PAINT, WM_PASTE, WM_SETFOCUS, WM_SIZE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_UNDO, WM_USER,
            WM_WINDOWPOSCHANGED, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_HSCROLL,
            WS_POPUP, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

// windows-sys omits RichEdit. This small ABI follows Microsoft's Richedit.h,
// which explicitly packs its public structures to four bytes on every target.
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_EXLIMITTEXT: u32 = WM_USER + 53;
const EM_EXSETSEL: u32 = WM_USER + 55;
const EM_FORMATRANGE: u32 = WM_USER + 57;
const EM_GETCHARFORMAT: u32 = WM_USER + 58;
const EM_GETPARAFORMAT: u32 = WM_USER + 61;
const EM_GETSELTEXT: u32 = WM_USER + 62;
const EM_PASTESPECIAL: u32 = WM_USER + 64;
const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETCHARFORMAT: u32 = WM_USER + 68;
const EM_SETPARAFORMAT: u32 = WM_USER + 71;
const EM_SETTARGETDEVICE: u32 = WM_USER + 72;
const EM_STREAMIN: u32 = WM_USER + 73;
const EM_STREAMOUT: u32 = WM_USER + 74;
const EM_SETUNDOLIMIT: u32 = WM_USER + 82;
const EM_REDO: u32 = WM_USER + 84;
const EM_STOPGROUPTYPING: u32 = WM_USER + 88;
const EM_AUTOURLDETECT: u32 = WM_USER + 91;
const EM_GETTEXTEX: u32 = WM_USER + 94;
const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
const EM_FINDTEXTEXW: u32 = WM_USER + 124;
const EM_SETEDITSTYLE: u32 = WM_USER + 204;
const EM_SETTYPOGRAPHYOPTIONS: u32 = WM_USER + 202;
/// TO_DEFAULTCOLOREMOJI | TO_DISPLAYFONTCOLOR: emoji take the emoji font's glyphs.
const EMOJI_TYPOGRAPHY: usize = 0x1000 | 0x2000;
const EM_SETZOOM: u32 = WM_USER + 225;
const SCF_SELECTION: usize = 1;
const SCF_ALL: usize = 4;
thread_local! {
    /// The character style held by Copy Style; RichEdit lives on the UI thread only.
    static COPIED_STYLE: Cell<Option<CharFormat>> = const { Cell::new(None) };
}
const SCF_DEFAULT: usize = 0;
const CFE_AUTOCOLOR: u32 = 0x4000_0000;
const EM_SETEVENTMASK: u32 = WM_USER + 69;
const ENM_CHANGE: isize = 1;
const EN_CHANGE: usize = 0x0300;
const EM_GETSCROLLPOS: u32 = WM_USER + 221;
const EM_SETSCROLLPOS: u32 = WM_USER + 222;
const CF_UNICODETEXT: usize = 13;
// ITextDocument::Undo arguments that pause and resume undo recording (tom.h).
const TOM_SUSPEND: i32 = -9_999_995;
const TOM_RESUME: i32 = -9_999_994;
const SF_RTF: usize = 2;
const SFF_SELECTION: usize = 0x8000;
const CFM_BOLD: u32 = 1;
const CFM_ITALIC: u32 = 2;
const CFM_UNDERLINE: u32 = 4;
const CFM_STRIKEOUT: u32 = 8;
const CFM_SIZE: u32 = 0x8000_0000;
const CFM_COLOR: u32 = 0x4000_0000;
const CFM_FACE: u32 = 0x2000_0000;
const CFM_BACKCOLOR: u32 = 0x0400_0000;
const CFM_SCRIPT: u32 = 0x0003_0000;
const PFM_STARTINDENT: u32 = 1;
const PFM_OFFSET: u32 = 4;
const PFM_ALIGNMENT: u32 = 8;
const PFM_NUMBERING: u32 = 0x20;
const PFM_LINESPACING: u32 = 0x100;
const PFM_NUMBERINGSTYLE: u32 = 0x2000;
const PFM_NUMBERINGTAB: u32 = 0x4000;
const PFM_NUMBERINGSTART: u32 = 0x8000;
const MAX_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
const SUBCLASS_ID: usize = 0x4558_4544;
/// Posted to a document host so focus moves into the editor after activation settles.
const FOCUS_EDITOR: u32 = WM_APP + 1;

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct CharRange {
    min: i32,
    max: i32,
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    foreground: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
    weight: u16,
    spacing: i16,
    background: u32,
    locale: u32,
    cookie: u32,
    style: i16,
    kerning: u16,
    underline_type: u8,
    animation: u8,
    author: u8,
    underline_color: u8,
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct ParaFormat {
    size: u32,
    mask: u32,
    numbering: u16,
    effects: u16,
    start_indent: i32,
    right_indent: i32,
    offset: i32,
    alignment: u16,
    tab_count: i16,
    tabs: [i32; 32],
    space_before: i32,
    space_after: i32,
    line_spacing: i32,
    style: i16,
    spacing_rule: u8,
    outline: u8,
    shading_weight: u16,
    shading_style: u16,
    numbering_start: u16,
    numbering_style: u16,
    numbering_tab: u16,
    border_space: u16,
    border_width: u16,
    borders: u16,
}
#[repr(C, packed(4))]
struct EditStream {
    cookie: usize,
    error: u32,
    callback: unsafe extern "system" fn(usize, *mut u8, i32, *mut i32) -> u32,
}
#[repr(C, packed(4))]
struct FindText {
    range: CharRange,
    text: *const u16,
    found: CharRange,
}
#[repr(C, packed(4))]
struct GetTextLength {
    flags: u32,
    codepage: u32,
}
#[repr(C, packed(4))]
struct GetText {
    bytes: u32,
    flags: u32,
    codepage: u32,
    default_char: *const u8,
    used_default: *mut i32,
}
#[repr(C, packed(4))]
struct FormatRange {
    dc: HDC,
    target_dc: HDC,
    content: RECT,
    page: RECT,
    characters: CharRange,
}

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
    /// Format > Font > Copy Style / Paste Style, shared by every document window.
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
    /// Ordinary RGB value, 0xRRGGBB, rather than native COLORREF byte order.
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
struct NativeState {
    shortcuts: RefCell<VecDeque<String>>,
    alive: Cell<bool>,
    /// GPUI window that owns the document host.
    owner: HWND,
    /// Borderless owned popup that contains the RichEdit control.
    host: HWND,
    /// Editor rectangle in the owner's client pixels.
    bounds: Cell<Option<(i32, i32, i32, i32)>>,
    visible: Cell<bool>,
    focus_on_show: Cell<bool>,
    /// Plain documents keep one style and paste text only; RichEdit itself
    /// stays in rich mode so source code can be colored.
    plain: Cell<bool>,
    /// Set by a paste so rich text pasted into a dark document can be adapted.
    pasted: Cell<bool>,
    /// Document background, behind the color emoji drawn over RichEdit's glyphs.
    paper: Cell<u32>,
    /// Whether the last paint drew emoji, and the selection it skipped them for.
    emoji: Cell<Option<crate::emoji::CharRange>>,
    wrap: Cell<Option<bool>>,
    margins: Cell<Option<(i32, i32)>>,
    chord: Cell<Option<std::time::Instant>>,
    consumed_key: Cell<bool>,
}
impl NativeState {
    /// Moves the host over the editor rectangle and shows it when the owner is on screen.
    fn place(&self) {
        let host = self.host;
        let Some((x, y, width, height)) = self.bounds.get() else {
            return;
        };
        if !self.alive.get() || host.is_null() {
            return;
        }
        let mut origin = POINT { x, y };
        // SAFETY: owner and host are live windows on this UI thread; origin is writable.
        unsafe {
            if ClientToScreen(self.owner, &mut origin) == 0 {
                return;
            }
            let shown =
                self.visible.get() && IsWindowVisible(self.owner) != 0 && IsIconic(self.owner) == 0;
            let was_shown = IsWindowVisible(host) != 0;
            // Owned popups do not follow an owner that is hidden without minimizing.
            let flags = SWP_NOACTIVATE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | if shown {
                    SWP_SHOWWINDOW
                } else {
                    SWP_HIDEWINDOW
                };
            SetWindowPos(
                host,
                ptr::null_mut(),
                origin.x,
                origin.y,
                width,
                height,
                flags,
            );
            if shown && !was_shown && self.focus_on_show.replace(false) {
                PostMessageW(host, FOCUS_EDITOR, 0, 0);
            }
        }
    }
}

/// Owns a RichEdit control inside an owned popup above its GPUI window.
///
/// GPUI renders through a topmost DirectComposition target with no GDI
/// redirection surface, so a child HWND would be hidden beneath it.
/// The thread-affinity marker forbids Send/Sync.
pub struct NativeEditor {
    hwnd: HWND,
    state: Box<NativeState>,
    read_only: Cell<bool>,
    /// Light color standing in for automatic text on a dark document.
    text_override: Cell<Option<u32>>,
    syntax_active: Cell<bool>,
    _ui_thread: PhantomData<Rc<()>>,
}
impl NativeEditor {
    pub fn new(parent: isize) -> Result<Self> {
        ensure!(parent != 0, "the document window is unavailable");
        static LIBRARY: OnceLock<bool> = OnceLock::new();
        let loaded = *LIBRARY.get_or_init(|| {
            // SAFETY: terminated module name; registration lives until process exit.
            unsafe { !LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null() }
        });
        ensure!(loaded, "Windows RichEdit could not be loaded");
        let owner = parent as HWND;
        let host_class = host_class()?;
        let empty = wide("");
        // SAFETY: registered class, live GPUI owner, and valid terminated strings.
        let host = unsafe {
            CreateWindowExW(
                0,
                host_class.as_ptr(),
                empty.as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                owner,
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        ensure!(
            !host.is_null(),
            "creating document host: {}",
            std::io::Error::last_os_error()
        );
        let class = wide("RICHEDIT50W");
        let style = WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | WS_HSCROLL
            | WS_CLIPSIBLINGS
            | (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | ES_NOHIDESEL) as u32;
        // SAFETY: registered class, live host parent, and valid terminated strings.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                empty.as_ptr(),
                style,
                0,
                0,
                1,
                1,
                host,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            let error = std::io::Error::last_os_error();
            // SAFETY: this newly created host has not escaped.
            unsafe {
                DestroyWindow(host);
            }
            bail!("creating document editor: {error}");
        }
        let state = Box::new(NativeState {
            shortcuts: RefCell::default(),
            alive: Cell::new(true),
            owner,
            host,
            bounds: Cell::new(None),
            visible: Cell::new(true),
            focus_on_show: Cell::new(false),
            plain: Cell::new(false),
            pasted: Cell::new(false),
            paper: Cell::new(0xffffff),
            emoji: Cell::new(None),
            wrap: Cell::new(None),
            margins: Cell::new(None),
            chord: Cell::new(None),
            consumed_key: Cell::new(false),
        });
        let data = (&*state as *const NativeState) as usize;
        // SAFETY: boxed state stays stable until both subclasses are removed in Drop;
        // its address doubles as a unique owner-subclass id per editor.
        let installed = unsafe {
            SetWindowSubclass(hwnd, Some(editor_subclass), SUBCLASS_ID, data) != 0
                && SetWindowSubclass(owner, Some(owner_subclass), data, data) != 0
        };
        if !installed {
            // SAFETY: the new windows have not escaped; removing an absent subclass is a no-op.
            unsafe {
                RemoveWindowSubclass(owner, Some(owner_subclass), data);
                DestroyWindow(host);
            }
            bail!("document keyboard integration could not be installed");
        }
        let editor = Self {
            hwnd,
            state,
            read_only: Cell::new(false),
            text_override: Cell::new(None),
            syntax_active: Cell::new(false),
            _ui_thread: PhantomData,
        };
        editor.send(EM_EXLIMITTEXT, 0, 64 * 1024 * 1024);
        // EN_CHANGE lets the window skip reading unchanged text every tick.
        editor.send(EM_SETEVENTMASK, 0, ENM_CHANGE);
        editor.send(EM_SETUNDOLIMIT, 1_000, 0);
        editor.send(EM_AUTOURLDETECT, 1, 0);
        // Windows' windowed RichEdit draws through GDI, so emoji stay monochrome, but this
        // binds them to Segoe UI Emoji and lets a RichEdit with color support use it.
        editor.send(
            EM_SETTYPOGRAPHYOPTIONS,
            EMOJI_TYPOGRAPHY,
            EMOJI_TYPOGRAPHY as isize,
        );
        editor.set_spellcheck(true);
        editor.set_wrap(true);
        editor.set_margins(8, 8);
        editor.set_background(0xffffff);
        editor.execute(EditCommand::Font("Helvetica".into()))?;
        editor.execute(EditCommand::FontSize(12.0))?;
        editor.mark_saved();
        Ok(editor)
    }
    /// Places the editor over a rectangle given in the owner's client pixels.
    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        let next = (x, y, width.max(0), height.max(0));
        if self.state.alive.get() && self.state.bounds.get() != Some(next) {
            self.state.bounds.set(Some(next));
            self.state.place();
        }
    }
    pub fn set_visible(&self, visible: bool) {
        self.state.visible.set(visible);
        if !self.state.alive.get() {
            return;
        }
        if visible {
            self.state.place();
        } else {
            // SAFETY: the host is owned on this UI thread.
            unsafe {
                ShowWindow(self.state.host, SW_HIDE);
            }
        }
    }
    /// Scrolls the selection into view (Find > Jump to Selection).
    pub fn scroll_to_selection(&self) {
        self.send(EM_SCROLLCARET, 0, 0);
    }
    pub fn focus(&self) {
        if !self.state.alive.get() {
            return;
        }
        // SAFETY: owned live HWNDs on their creating thread.
        unsafe {
            if IsWindowVisible(self.state.host) != 0 {
                SetFocus(self.hwnd);
            } else {
                // A hidden host cannot take activation; focus once it first appears.
                self.state.focus_on_show.set(true);
            }
        }
    }
    pub fn take_shortcut(&self) -> Option<String> {
        self.state.shortcuts.borrow_mut().pop_front()
    }
    pub fn text(&self) -> Result<String> {
        let len = self.text_length();
        ensure!(
            len <= MAX_DOCUMENT_BYTES / 2,
            "the document exceeds the supported text size"
        );
        let mut buffer = vec![0_u16; len + 1];
        let options = GetText {
            bytes: u32::try_from(buffer.len() * 2)?,
            flags: 0,
            codepage: 1200,
            default_char: ptr::null(),
            used_default: ptr::null_mut(),
        };
        let copied = self.send(
            EM_GETTEXTEX,
            (&options as *const GetText) as usize,
            buffer.as_mut_ptr() as isize,
        );
        ensure!(
            copied >= 0 && copied as usize <= len,
            "Windows could not read the complete document"
        );
        // Lossy: a lone surrogate must not make saving or closing impossible.
        Ok(normalize_newlines(&String::from_utf16_lossy(
            &buffer[..copied as usize],
        )))
    }
    pub fn set_text(&self, text: &str) -> Result<()> {
        ensure!(
            !text.contains('\0'),
            "plain text contains an embedded NUL character"
        );
        ensure!(
            text.len() <= MAX_DOCUMENT_BYTES,
            "the document is too large"
        );
        let value = wide(&text.replace("\r\n", "\r").replace('\n', "\r"));
        // EM_REPLACESEL, unlike WM_SETTEXT, never parses text that starts with
        // `{\rtf`, and the new text keeps a plain document's single style.
        let read_only = self.read_only.get();
        if read_only {
            self.send(EM_SETREADONLY, 0, 0);
        }
        self.set_selection(CharRange { min: 0, max: -1 });
        self.send(EM_REPLACESEL, 0, value.as_ptr() as isize);
        self.set_selection(CharRange::default());
        if read_only {
            self.send(EM_SETREADONLY, 1, 0);
        }
        self.send(EM_EMPTYUNDOBUFFER, 0, 0);
        Ok(())
    }
    pub fn rtf(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut stream = EditStream {
            cookie: (&mut bytes as *mut Vec<u8>) as usize,
            error: 0,
            callback: stream_out,
        };
        self.send(
            EM_STREAMOUT,
            SF_RTF,
            (&mut stream as *mut EditStream) as isize,
        );
        let error = stream.error;
        ensure!(
            error == 0,
            "Windows could not serialize the complete document (error {error})"
        );
        ensure!(
            !bytes.is_empty(),
            "Windows returned an empty rich-text stream"
        );
        Ok(bytes)
    }
    pub fn set_rtf(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_DOCUMENT_BYTES,
            "the rich-text document is too large"
        );
        ensure!(
            bytes.starts_with(b"{\\rtf"),
            "the document is not a valid RTF stream"
        );
        let previous = self.rtf()?;
        if let Err(error) = self.stream_rtf(bytes, false) {
            self.stream_rtf(&previous, false)
                .context("loading failed and Windows could not restore the previous document")?;
            return Err(error);
        }
        self.send(EM_EMPTYUNDOBUFFER, 0, 0);
        self.syntax_active.set(false);
        self.adapt_automatic_text()
    }
    pub fn is_modified(&self) -> bool {
        self.send(EM_GETMODIFY, 0, 0) != 0
    }
    pub fn mark_saved(&self) {
        self.send(EM_SETMODIFY, 0, 0);
    }
    /// Clears the native change flag after a durable snapshot, keeping document dirty state separate.
    pub fn acknowledge_buffered_changes(&self) {
        self.send(EM_SETMODIFY, 0, 0);
    }
    pub fn mark_modified(&self) {
        self.send(EM_SETMODIFY, 1, 0);
    }
    pub fn set_read_only(&self, read_only: bool) {
        self.send(EM_SETREADONLY, usize::from(read_only), 0);
        self.read_only.set(read_only);
    }
    pub fn selection(&self) -> Range<usize> {
        let range = self.native_selection();
        range.min.max(0) as usize..range.max.max(0) as usize
    }
    pub fn select_range(&self, range: Range<usize>) -> Result<()> {
        ensure!(
            range.start <= range.end && range.end <= self.text_length(),
            "the requested selection is outside the document"
        );
        self.set_selection(CharRange {
            min: i32::try_from(range.start)?,
            max: i32::try_from(range.end)?,
        });
        Ok(())
    }
    pub fn selected_text(&self) -> Result<String> {
        let selection = self.native_selection();
        let len = selection.max.saturating_sub(selection.min).max(0) as usize;
        let mut buffer = vec![0_u16; len + 1];
        let copied = self.send(EM_GETSELTEXT, 0, buffer.as_mut_ptr() as isize);
        ensure!(
            copied >= 0 && copied as usize <= len,
            "Windows could not read the selected text"
        );
        Ok(normalize_newlines(&String::from_utf16_lossy(
            &buffer[..copied as usize],
        )))
    }
    pub fn execute(&self, command: EditCommand) -> Result<()> {
        if self.read_only.get() && !matches!(command, EditCommand::Copy | EditCommand::SelectAll) {
            bail!("the document is locked");
        }
        match command {
            EditCommand::Undo => {
                self.send(WM_UNDO, 0, 0);
            }
            EditCommand::Redo => {
                self.send(EM_REDO, 0, 0);
            }
            EditCommand::Cut => {
                self.send(WM_CUT, 0, 0);
            }
            EditCommand::Copy => {
                self.send(WM_COPY, 0, 0);
            }
            EditCommand::Paste => {
                self.send(WM_PASTE, 0, 0);
            }
            EditCommand::PastePlain => {
                self.send(EM_PASTESPECIAL, 13, 0);
            }
            EditCommand::Delete => {
                self.send(WM_CLEAR, 0, 0);
            }
            EditCommand::SelectAll => self.set_selection(CharRange { min: 0, max: -1 }),
            EditCommand::Bold => self.toggle_effect(CFM_BOLD, CFM_BOLD)?,
            EditCommand::Italic => self.toggle_effect(CFM_ITALIC, CFM_ITALIC)?,
            EditCommand::Underline => self.toggle_effect(CFM_UNDERLINE, CFM_UNDERLINE)?,
            EditCommand::Strikethrough => self.toggle_effect(CFM_STRIKEOUT, CFM_STRIKEOUT)?,
            EditCommand::Superscript => self.toggle_effect(CFM_SCRIPT, 0x0002_0000)?,
            EditCommand::Subscript => self.toggle_effect(CFM_SCRIPT, 0x0001_0000)?,
            EditCommand::CopyStyle => COPIED_STYLE.set(Some(self.char_format())),
            EditCommand::PasteStyle => {
                let style = COPIED_STYLE
                    .get()
                    .context("Copy a style first with Format > Font > Copy Style")?;
                self.apply_char(style)?;
            }
            EditCommand::Font(font) => {
                ensure!(
                    !font.is_empty() && !font.contains('\0'),
                    "choose a valid font name"
                );
                let name: Vec<u16> = font.encode_utf16().collect();
                ensure!(
                    name.len() < 32,
                    "the font name is too long for Windows RichEdit"
                );
                let mut format = self.empty_char_format();
                format.mask = CFM_FACE;
                format.face[..name.len()].copy_from_slice(&name);
                self.apply_char(format)?;
            }
            EditCommand::FontSize(points) => {
                ensure!(
                    points.is_finite() && (1.0..=1638.0).contains(&points),
                    "font size must be between 1 and 1638 points"
                );
                let mut format = self.empty_char_format();
                format.mask = CFM_SIZE;
                format.height = (points * 20.0).round() as i32;
                self.apply_char(format)?;
            }
            EditCommand::Foreground(color) => {
                let mut format = self.empty_char_format();
                format.mask = CFM_COLOR;
                format.foreground = color_ref(color);
                self.apply_char(format)?;
            }
            EditCommand::Background(color) => {
                let mut format = self.empty_char_format();
                format.mask = CFM_BACKCOLOR;
                format.background = color_ref(color);
                self.apply_char(format)?;
            }
            EditCommand::Align(alignment) => {
                let mut format = self.empty_para_format();
                format.mask = PFM_ALIGNMENT;
                format.alignment = match alignment {
                    Alignment::Left => 1,
                    Alignment::Right => 2,
                    Alignment::Center => 3,
                    Alignment::Justify => 4,
                };
                self.apply_para(format)?;
            }
            EditCommand::LineSpacing(lines) => {
                ensure!(
                    lines.is_finite() && (0.5..=10.0).contains(&lines),
                    "line spacing must be between 0.5 and 10"
                );
                let mut format = self.empty_para_format();
                format.mask = PFM_LINESPACING;
                format.spacing_rule = 5;
                format.line_spacing = (lines * 20.0).round() as i32;
                self.apply_para(format)?;
            }
            EditCommand::Indent(delta) => {
                let mut format = self.paragraph_format();
                format.mask = PFM_STARTINDENT;
                format.start_indent = format.start_indent.saturating_add(delta).clamp(0, 28_800);
                self.apply_para(format)?;
            }
            EditCommand::FirstLineIndent(twips) => {
                let mut format = self.paragraph_format();
                let other_lines = format.start_indent.saturating_add(format.offset);
                format.mask = PFM_STARTINDENT | PFM_OFFSET;
                format.start_indent = twips.clamp(0, 28_800);
                format.offset = other_lines.saturating_sub(format.start_indent);
                self.apply_para(format)?;
            }
            EditCommand::TabStop(twips) => {
                ensure!(
                    (0..=28_800).contains(&twips),
                    "the tab stop must fall within the document width"
                );
                let mut format = self.paragraph_format();
                let original = format.tabs;
                let count = format.tab_count.clamp(0, 32) as usize;
                let mut tabs: Vec<i32> = original[..count]
                    .iter()
                    .copied()
                    .filter(|tab| *tab >= 0)
                    .collect();
                if !tabs.contains(&twips) {
                    tabs.push(twips);
                }
                tabs.sort_unstable();
                ensure!(
                    tabs.len() <= 32,
                    "a paragraph supports at most 32 tab stops"
                );
                format.mask = 0x10;
                format.tab_count = tabs.len() as i16;
                format.tabs = [0; 32];
                format.tabs[..tabs.len()].copy_from_slice(&tabs);
                self.apply_para(format)?;
            }
            EditCommand::Direction(right_to_left) => {
                let mut format = self.paragraph_format();
                format.mask = 0x0001_0000;
                format.effects = (format.effects & !1) | u16::from(right_to_left);
                self.apply_para(format)?;
            }
            EditCommand::List(kind) => {
                let mut format = self.empty_para_format();
                format.mask = PFM_NUMBERING
                    | PFM_NUMBERINGSTART
                    | PFM_NUMBERINGSTYLE
                    | PFM_NUMBERINGTAB
                    | PFM_OFFSET;
                format.numbering = match kind {
                    ListKind::None => 0,
                    ListKind::Bullet => 1,
                    ListKind::Number => 2,
                };
                format.numbering_start = 1;
                format.numbering_style = 0x200;
                format.numbering_tab = 360;
                format.offset = if kind == ListKind::None { 0 } else { 360 };
                self.apply_para(format)?;
            }
            EditCommand::ConvertCase(case) => {
                self.transform_case(case)?;
            }
        }
        Ok(())
    }
    pub fn formatting(&self) -> FormatState {
        let format = self.char_format();
        let face = format.face;
        let end = face
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(face.len());
        let paragraph = self.paragraph_format();
        FormatState {
            font: String::from_utf16_lossy(&face[..end]),
            size: format.height as f32 / 20.0,
            bold: format.mask & CFM_BOLD != 0 && format.effects & CFM_BOLD != 0,
            italic: format.mask & CFM_ITALIC != 0 && format.effects & CFM_ITALIC != 0,
            underline: format.mask & CFM_UNDERLINE != 0 && format.effects & CFM_UNDERLINE != 0,
            strikethrough: format.mask & CFM_STRIKEOUT != 0 && format.effects & CFM_STRIKEOUT != 0,
            foreground: if format.effects & CFE_AUTOCOLOR != 0 {
                0
            } else {
                color_ref(format.foreground)
            },
            background: if format.mask & CFM_BACKCOLOR != 0 && format.effects & 0x0400_0000 == 0 {
                color_ref(format.background)
            } else {
                0xffffff
            },
            alignment: match paragraph.alignment {
                2 => Alignment::Right,
                3 => Alignment::Center,
                4 => Alignment::Justify,
                _ => Alignment::Left,
            },
        }
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
        let text = wide(&needle.replace("\r\n", "\r").replace('\n', "\r"));
        let selection = self.native_selection();
        let end = i32::try_from(self.text_length())?;
        let flags = usize::from(!backwards)
            | if match_case { 4 } else { 0 }
            | if whole_word { 2 } else { 0 };
        let ranges = if backwards {
            [(selection.min, 0), (end, selection.min)]
        } else {
            [(selection.max, end), (0, selection.max)]
        };
        for (min, max) in ranges {
            let mut query = FindText {
                range: CharRange { min, max },
                text: text.as_ptr(),
                found: CharRange::default(),
            };
            if self.send(
                EM_FINDTEXTEXW,
                flags,
                (&mut query as *mut FindText) as isize,
            ) >= 0
            {
                self.set_selection(query.found);
                self.send(EM_SCROLLCARET, 0, 0);
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn replace_current(&self, text: &str) -> Result<()> {
        ensure!(!self.read_only.get(), "the document is locked");
        ensure!(
            !text.contains('\0'),
            "the replacement contains a NUL character"
        );
        ensure!(
            text.len() <= MAX_DOCUMENT_BYTES,
            "the replacement is too large"
        );
        let value = wide(&text.replace("\r\n", "\r").replace('\n', "\r"));
        self.send(EM_REPLACESEL, 1, value.as_ptr() as isize);
        Ok(())
    }
    pub fn edit_lines(&self, edit: crate::editing::TextEdit) -> Result<()> {
        use crate::editing::EditPiece;
        ensure!(!self.read_only.get(), "the document is locked");
        let previous_selection = self.selection();
        let previous_modified = self.is_modified();
        let original_length = self.text_length();
        let expected_length = original_length - (edit.range.end - edit.range.start)
            + edit.replacement.encode_utf16().count();
        let original = self.rtf_range(edit.range.clone())?;
        let pieces: Vec<_> = edit
            .pieces
            .into_iter()
            .map(|piece| match piece {
                EditPiece::Original(range) => self
                    .rtf_range(range)
                    .map(crate::document::DocumentContent::Rich),
                EditPiece::Text(text) => Ok(crate::document::DocumentContent::Plain(text)),
            })
            .collect::<Result<_>>()?;
        self.select_range(previous_selection.clone())?;
        let _group = self.begin_collection()?;
        let result = (|| -> Result<()> {
            self.select_range(edit.range.clone())?;
            if pieces.is_empty() {
                self.replace_current("")?;
            }
            for piece in pieces {
                match piece {
                    crate::document::DocumentContent::Plain(text) => self.replace_current(&text)?,
                    crate::document::DocumentContent::Rich(rtf) => self.read_rtf(&rtf, true)?,
                }
                let end = self.selection().end;
                self.select_range(end..end)?;
            }
            ensure!(
                self.text_length() == expected_length,
                "Windows did not retain the exact line edit; the original document will be restored"
            );
            self.select_range(edit.selection)?;
            self.mark_modified();
            Ok(())
        })();
        if let Err(error) = result {
            let unchanged = original_length - (edit.range.end - edit.range.start);
            let current_end = edit.range.start + self.text_length().saturating_sub(unchanged);
            self.select_range(edit.range.start..current_end)?;
            self.read_rtf(&original, true)
                .context("Restoring the document after the line edit failed")?;
            self.select_range(previous_selection)?;
            if !previous_modified {
                self.mark_saved();
            }
            return Err(error);
        }
        Ok(())
    }
    fn rtf_range(&self, range: Range<usize>) -> Result<Vec<u8>> {
        let selection = self.native_selection();
        self.select_range(range)?;
        let mut bytes = Vec::new();
        let mut stream = EditStream {
            cookie: (&mut bytes as *mut Vec<u8>) as usize,
            error: 0,
            callback: stream_out,
        };
        self.send(
            EM_STREAMOUT,
            SF_RTF | 0x8000,
            (&mut stream as *mut EditStream) as isize,
        );
        self.set_selection(selection);
        ensure!(
            stream.error == 0 && !bytes.is_empty(),
            "Windows could not copy the complete rich-text range"
        );
        Ok(bytes)
    }
    pub fn scroll_editor(&self, amount: i32, page: bool) {
        if page {
            self.send(0x0115, if amount < 0 { 2 } else { 3 }, 0);
        } else {
            self.send(0x00b6, 0, amount as isize);
        }
    }
    pub fn replace_all(
        &self,
        needle: &str,
        replacement: &str,
        match_case: bool,
        whole_word: bool,
    ) -> Result<usize> {
        ensure!(!self.read_only.get(), "the document is locked");
        ensure!(
            !needle.contains('\0') && !replacement.contains('\0'),
            "find and replace text cannot contain NUL characters"
        );
        ensure!(
            replacement.len() <= MAX_DOCUMENT_BYTES,
            "the replacement is too large"
        );
        if needle.is_empty() {
            return Ok(0);
        }
        let needle = wide(&needle.replace("\r\n", "\r").replace('\n', "\r"));
        let replacement = wide(&replacement.replace("\r\n", "\r").replace('\n', "\r"));
        let flags = 1 | if match_case { 4 } else { 0 } | if whole_word { 2 } else { 0 };
        let mut matches = Vec::new();
        let mut start = 0;
        loop {
            let mut query = FindText {
                range: CharRange {
                    min: start,
                    max: -1,
                },
                text: needle.as_ptr(),
                found: CharRange::default(),
            };
            if self.send(
                EM_FINDTEXTEXW,
                flags,
                (&mut query as *mut FindText) as isize,
            ) < 0
            {
                break;
            }
            let found = query.found;
            let next = found.max;
            ensure!(
                next > start,
                "Windows returned a search match that did not advance"
            );
            matches
                .try_reserve(1)
                .context("allocating search matches")?;
            matches.push(found);
            start = next;
        }
        if matches.is_empty() {
            return Ok(0);
        }
        let existing = self.text_length();
        let removed: usize = matches
            .iter()
            .map(|range| (range.max - range.min) as usize)
            .sum();
        let inserted = matches
            .len()
            .checked_mul(replacement.len().saturating_sub(1))
            .context("the replacement result is too large")?;
        let projected = existing
            .saturating_sub(removed)
            .checked_add(inserted)
            .context("the replacement result is too large")?;
        ensure!(
            projected <= MAX_DOCUMENT_BYTES / 2,
            "the replacement result is too large"
        );
        let _edit = self.begin_collection()?;
        let old_selection = self.native_selection();
        self.send(EM_STOPGROUPTYPING, 0, 0);
        for &range in matches.iter().rev() {
            self.set_selection(range);
            self.send(EM_REPLACESEL, 1, replacement.as_ptr() as isize);
        }
        let len = i32::try_from(self.text_length())?;
        self.set_selection(CharRange {
            min: old_selection.min.min(len),
            max: old_selection.max.min(len),
        });
        self.send(EM_STOPGROUPTYPING, 0, 0);
        Ok(matches.len())
    }
    pub fn set_auto_links(&self, enabled: bool) {
        self.send(EM_AUTOURLDETECT, usize::from(enabled), 0);
    }
    pub fn set_plain_text_mode(&self, plain: bool) -> Result<()> {
        ensure!(self.state.alive.get(), "the document window has closed");
        if self.state.plain.replace(plain) == plain || !plain {
            return Ok(());
        }
        // Rich to plain drops every style, paragraph format, and attachment.
        let modified = self.is_modified();
        let text = self.text()?;
        self.set_selection(CharRange::default());
        let mut font = self.char_format();
        font.mask &= CFM_FACE | CFM_SIZE;
        self.stream_rtf(b"{\\rtf1 }", false)?;
        self.send(
            EM_SETCHARFORMAT,
            SCF_DEFAULT,
            (&font as *const CharFormat) as isize,
        );
        self.send(
            EM_SETCHARFORMAT,
            SCF_ALL,
            (&font as *const CharFormat) as isize,
        );
        self.set_text(&text)?;
        self.syntax_active.set(false);
        self.adapt_automatic_text()?;
        if modified {
            self.mark_modified();
        } else {
            self.mark_saved();
        }
        Ok(())
    }
    /// True once after the user changed the text (EN_CHANGE), so callers can
    /// skip reading an unchanged document.
    pub fn take_text_changed(&self) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, SetWindowLongPtrW};
        // SAFETY: the host is a live window owned by this UI thread.
        self.state.alive.get()
            && unsafe { SetWindowLongPtrW(self.state.host, GWLP_USERDATA, 0) } != 0
    }
    /// Colors runs of a source file without touching undo, selection,
    /// scrolling, or the modified flag.
    pub fn apply_spans(&self, spans: &[crate::syntax::Span]) -> Result<()> {
        if spans.is_empty() {
            return Ok(());
        }
        let _quiet = QuietEdit::new(self)?;
        let document = self.tom_document()?;
        let length = self.text_length();
        for span in spans {
            let min = i32::try_from(span.start.min(length))?;
            let max = i32::try_from(span.start.saturating_add(span.len).min(length))?;
            if min >= max {
                continue;
            }
            // SAFETY: the range is bounded by this live native document.
            unsafe {
                let font = document.Range(min, max)?.GetFont()?;
                font.Reset(tomApplyTmp)?;
                font.SetForeColor(color_ref(span.color) as i32)?;
                font.Reset(tomApplyNow)?;
            }
            if self.state.plain.get() {
                self.set_selection(CharRange { min, max });
                let mut format = self.empty_char_format();
                format.mask = CFM_BOLD | CFM_ITALIC;
                if span.bold {
                    format.effects |= CFM_BOLD;
                }
                if span.italic {
                    format.effects |= CFM_ITALIC;
                }
                ensure!(
                    self.send(
                        EM_SETCHARFORMAT,
                        SCF_SELECTION,
                        (&format as *const CharFormat) as isize
                    ) != 0,
                    "Windows could not apply syntax emphasis"
                );
            }
        }
        self.syntax_active.set(true);
        Ok(())
    }
    pub fn clear_spans(&self) -> Result<()> {
        if self.syntax_active.replace(false) {
            let _quiet = QuietEdit::new(self)?;
            // Syntax weight/emphasis is only applied in plain documents.
            let mut format = self.empty_char_format();
            format.mask = CFM_BOLD | CFM_ITALIC;
            self.send(
                EM_SETCHARFORMAT,
                SCF_ALL,
                (&format as *const CharFormat) as isize,
            );
            self.send(
                EM_SETCHARFORMAT,
                SCF_DEFAULT,
                (&format as *const CharFormat) as isize,
            );
            self.refresh_temporary_colors(self.text_override.get())?;
        }
        Ok(())
    }
    /// Changes only the temporary display layer. Original colors remain in RTF,
    /// clipboard data, undo history and recovery snapshots.
    pub fn set_text_override(&self, color: Option<u32>) -> Result<()> {
        if self.text_override.get() != color {
            self.refresh_temporary_colors(color)?;
            self.text_override.set(color);
        }
        Ok(())
    }
    pub fn take_pasted(&self) -> bool {
        self.state.pasted.replace(false)
    }
    pub fn adapt_automatic_text(&self) -> Result<()> {
        if let Some(color) = self.text_override.get() {
            self.refresh_temporary_colors(Some(color))?;
        }
        Ok(())
    }
    fn refresh_temporary_colors(&self, automatic: Option<u32>) -> Result<()> {
        let document = self.tom_document()?;
        let _quiet = QuietEdit::new(self)?;
        self.color_runs(&document, 0, i32::try_from(self.text_length())?, automatic)
    }
    fn color_runs(
        &self,
        document: &ITextDocument,
        start: i32,
        end: i32,
        automatic: Option<u32>,
    ) -> Result<()> {
        // SAFETY: the live UI-thread document owns the checked UTF-16 range.
        let font = unsafe { document.Range(start, end)?.GetFont()? };
        // TOM reads the underlying format, not the temporary display format.
        let original = unsafe { font.GetForeColor()? };
        if original == tomUndefined.0 && end - start > 1 {
            let middle = start + (end - start) / 2;
            self.color_runs(document, start, middle, automatic)?;
            self.color_runs(document, middle, end, automatic)?;
        } else if original != tomUndefined.0 {
            let foreground = if original == tomAutoColor.0 {
                automatic.map_or(tomAutoColor.0, |rgb| color_ref(rgb) as i32)
            } else {
                original
            };
            // SAFETY: tomApplyTmp is display-only and never changes serialized formatting.
            unsafe {
                font.Reset(tomApplyTmp)?;
                font.SetForeColor(foreground)?;
                font.Reset(tomApplyNow)?;
            }
        }
        Ok(())
    }
    /// Creates an independent, read-only view without disturbing this editor's undo state.
    pub fn preview(&self, rtf: &[u8]) -> Result<Self> {
        let preview = Self::new(self.state.owner as isize)?;
        preview.set_visible(false);
        preview.set_rtf(rtf)?;
        preview.set_read_only(true);
        preview.mark_saved();
        Ok(preview)
    }
    pub fn set_zoom(&self, percent: u16) {
        self.send(EM_SETZOOM, usize::from(percent.clamp(10, 500)), 100);
    }
    pub fn set_wrap(&self, wrap: bool) {
        if self.state.wrap.replace(Some(wrap)) == Some(wrap) {
            return;
        }
        self.send(EM_SETTARGETDEVICE, 0, if wrap { 0 } else { 1 });
        // SAFETY: this modifies only the live owned child window.
        if self.state.alive.get() {
            unsafe {
                ShowScrollBar(self.hwnd, SB_HORZ, i32::from(!wrap));
            }
        }
    }
    pub fn set_margins(&self, left: i32, right: i32) {
        if self.state.margins.replace(Some((left, right))) == Some((left, right)) {
            return;
        }
        let left = left.clamp(0, u16::MAX as i32) as u32;
        let right = right.clamp(0, u16::MAX as i32) as u32;
        self.send(EM_SETMARGINS, 3, (left | (right << 16)) as isize);
    }
    pub fn set_background(&self, color: u32) {
        if self.state.paper.replace(color) == color {
            return;
        }
        self.send(EM_SETBKGNDCOLOR, 0, color_ref(color) as isize);
    }
    pub fn set_spellcheck(&self, enabled: bool) {
        let mask = 0x0001_0000 | 0x0080_0000;
        let value = 0x0001_0000 | if enabled { 0x0080_0000 } else { 0 };
        self.send(EM_SETEDITSTYLE, value, mask);
    }
    pub fn insert_table(&self, rows: u16, columns: u16) -> Result<()> {
        ensure!(
            (1..=100).contains(&rows) && (1..=63).contains(&columns),
            "a table must have 1–100 rows and 1–63 columns"
        );
        let width = 9_000 / i32::from(columns);
        let mut rtf = String::from("{\\rtf1\\ansi\\deff0 ");
        for _ in 0..rows {
            rtf.push_str("\\trowd\\trgaph108\\trleft0");
            for column in 1..=columns {
                write!(
                    rtf,
                    "\\clbrdrt\\brdrs\\brdrw10\\clbrdrl\\brdrs\\brdrw10\\clbrdrb\\brdrs\\brdrw10\\clbrdrr\\brdrs\\brdrw10\\cellx{}",
                    i32::from(column) * width
                )?;
            }
            rtf.push_str("\\intbl ");
            for _ in 0..columns {
                rtf.push_str("\\cell ");
            }
            rtf.push_str("\\row ");
        }
        rtf.push_str("\\pard\\par}");
        self.stream_rtf(rtf.as_bytes(), true)
    }
    pub fn insert_link(&self, url: &str, label: &str) -> Result<()> {
        ensure!(
            !url.is_empty() && !url.chars().any(|c| matches!(c, '\r' | '\n' | '\0' | '"')),
            "enter a valid link without quotes or line breaks"
        );
        let label = if label.is_empty() { url } else { label };
        let rtf = format!(
            "{{\\rtf1\\ansi{{\\field{{\\*\\fldinst HYPERLINK \"{}\"}}{{\\fldrslt{{\\ul {}}}}}}}}}",
            rtf_escape(url),
            rtf_escape(label)
        );
        self.stream_rtf(rtf.as_bytes(), true)
    }
    pub fn insert_image(&self, path: &Path) -> Result<()> {
        let mut reader = image::ImageReader::open(path)
            .with_context(|| format!("opening image {}", path.display()))?
            .with_guessed_format()
            .context("identifying the image format")?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().context("decoding the image")?.to_rgba8();
        let (width, height) = image.dimensions();
        let dib = bitmap_dib(width, height, image.as_raw())?;
        let mut rtf = format!(
            "{{\\rtf1\\ansi{{\\pict\\dibitmap0\\wbmbitspixel24\\wbmplanes1\\wbmwidthbytes{}\\picw{}\\pich{}\\picwgoal{}\\pichgoal{} ",
            (u64::from(width) * 3 + 3) & !3,
            width,
            height,
            width.saturating_mul(15),
            height.saturating_mul(15)
        );
        rtf.try_reserve(dib.len().saturating_mul(2))
            .context("allocating image data")?;
        for byte in dib {
            write!(rtf, "{byte:02x}")?;
        }
        rtf.push_str("}}");
        self.stream_rtf(rtf.as_bytes(), true)
    }
    pub fn print(&self, page: &PageSettings) -> Result<()> {
        validate_page(page)?;
        let mut resources = PrintResources {
            editor: self.hwnd,
            dialog: PRINTDLGW {
                lStructSize: size_of::<PRINTDLGW>() as u32,
                hwndOwner: self.state.owner,
                Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE,
                nFromPage: 1,
                nToPage: 1,
                nMinPage: 1,
                nMaxPage: u16::MAX,
                nCopies: 1,
                ..PRINTDLGW::default()
            },
            active: false,
        };
        // SAFETY: initialized dialog structure; returned handles are owned by resources.
        if unsafe { PrintDlgW(&mut resources.dialog) } == 0 {
            // SAFETY: called immediately after the native common dialog returns.
            let error = unsafe { CommDlgExtendedError() };
            ensure!(error == 0, "the print dialog failed (error {error})");
            return Ok(());
        }
        let range = if resources.dialog.Flags & PD_SELECTION != 0 {
            self.native_selection()
        } else {
            CharRange {
                min: 0,
                max: i32::try_from(self.text_length())?,
            }
        };
        let pages = if resources.dialog.Flags & PD_PAGENUMS != 0 {
            Some(u32::from(resources.dialog.nFromPage)..=u32::from(resources.dialog.nToPage))
        } else {
            None
        };
        self.render_print_job(&mut resources, page, range, pages, ptr::null())
    }
    pub fn export_pdf(&self, path: &Path, page: &PageSettings) -> Result<()> {
        use std::os::windows::ffi::OsStrExt as _;
        validate_page(page)?;
        let path = std::path::absolute(path).context("resolving the PDF output path")?;
        let output: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        ensure!(
            !output[..output.len() - 1].contains(&0),
            "the PDF path contains a NUL character"
        );
        let driver = wide("WINSPOOL");
        let printer = wide("Microsoft Print to PDF");
        let settings = printer_mode(page)?;
        // SAFETY: terminated printer names and default printer configuration.
        let dc = unsafe {
            windows_sys::Win32::Graphics::Gdi::CreateDCW(
                driver.as_ptr(),
                printer.as_ptr(),
                ptr::null(),
                &settings,
            )
        };
        ensure!(
            !dc.is_null(),
            "Microsoft Print to PDF is not installed or could not be opened"
        );
        let mut resources = PrintResources {
            editor: self.hwnd,
            dialog: PRINTDLGW {
                hDC: dc,
                ..PRINTDLGW::default()
            },
            active: false,
        };
        let range = CharRange {
            min: 0,
            max: i32::try_from(self.text_length())?,
        };
        self.render_print_job(&mut resources, page, range, None, output.as_ptr())
            .with_context(|| format!("exporting PDF {}", path.display()))
    }
    fn render_print_job(
        &self,
        resources: &mut PrintResources,
        page: &PageSettings,
        selection: CharRange,
        selected_pages: Option<std::ops::RangeInclusive<u32>>,
        output: *const u16,
    ) -> Result<()> {
        let _appearance = PrintAppearance::new(self);
        let dc = resources.dialog.hDC;
        ensure!(
            !dc.is_null(),
            "the selected printer returned no drawing context"
        );
        // SAFETY: resources owns the printer DC until this function returns.
        let (dpi_x, dpi_y, width_px, height_px, offset_x, offset_y) = unsafe {
            (
                GetDeviceCaps(dc, LOGPIXELSX as i32),
                GetDeviceCaps(dc, LOGPIXELSY as i32),
                GetDeviceCaps(dc, PHYSICALWIDTH as i32),
                GetDeviceCaps(dc, PHYSICALHEIGHT as i32),
                GetDeviceCaps(dc, PHYSICALOFFSETX as i32),
                GetDeviceCaps(dc, PHYSICALOFFSETY as i32),
            )
        };
        ensure!(
            dpi_x > 0 && dpi_y > 0 && width_px > 0 && height_px > 0,
            "the selected printer returned invalid paper dimensions"
        );
        let width = page.width_twips.min(width_px.saturating_mul(1440) / dpi_x);
        let height = page
            .height_twips
            .min(height_px.saturating_mul(1440) / dpi_y);
        ensure!(
            page.margin_left + page.margin_right < width
                && page.margin_top + page.margin_bottom < height,
            "the margins do not fit the selected printer's paper"
        );
        let title = wide("ExEditor document");
        let info = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: title.as_ptr(),
            lpszOutput: output,
            ..DOCINFOW::default()
        };
        // SAFETY: live printer DC and strings that outlive the synchronous print job.
        ensure!(
            unsafe { StartDocW(dc, &info) } > 0,
            "the print job could not be started"
        );
        resources.active = true;
        let mut range = FormatRange {
            dc,
            target_dc: dc,
            content: RECT {
                left: page.margin_left,
                top: page.margin_top,
                right: width - page.margin_right,
                bottom: height - page.margin_bottom,
            },
            page: RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            characters: selection,
        };
        let mut page_number = 1_u32;
        loop {
            let selected = selected_pages
                .as_ref()
                .is_none_or(|pages| pages.contains(&page_number));
            if selected {
                // SAFETY: an active document job exists on this owned printer DC.
                ensure!(
                    unsafe { StartPage(dc) } > 0,
                    "the printer could not start page {page_number}"
                );
                // SAFETY: convert the printer's printable-area origin to paper origin.
                unsafe {
                    SetViewportOrgEx(dc, -offset_x, -offset_y, ptr::null_mut());
                }
            }
            let next = if range.characters.min < range.characters.max {
                self.send(
                    EM_FORMATRANGE,
                    usize::from(selected),
                    (&range as *const FormatRange) as isize,
                )
            } else {
                range.characters.max as isize
            };
            if selected {
                // SAFETY: the corresponding page was started above.
                ensure!(
                    unsafe { EndPage(dc) } > 0,
                    "the printer could not finish page {page_number}"
                );
            }
            if next >= range.characters.max as isize {
                break;
            }
            ensure!(
                next > range.characters.min as isize,
                "Windows could not lay out page {page_number}"
            );
            range.characters.min = i32::try_from(next)?;
            page_number += 1;
        }
        // SAFETY: the document job has finished all requested pages.
        ensure!(
            unsafe { EndDoc(dc) } > 0,
            "the printer could not finish the document"
        );
        resources.active = false;
        Ok(())
    }
    fn send(&self, message: u32, wparam: usize, lparam: isize) -> isize {
        if !self.state.alive.get() {
            return 0;
        }
        // SAFETY: this UI-thread-owned HWND is live, and all message buffers
        // provided by callers outlive this synchronous call.
        unsafe { SendMessageW(self.hwnd, message, wparam, lparam) }
    }
    fn text_length(&self) -> usize {
        let options = GetTextLength {
            flags: 2 | 8,
            codepage: 1200,
        };
        self.send(
            EM_GETTEXTLENGTHEX,
            (&options as *const GetTextLength) as usize,
            0,
        )
        .max(0) as usize
    }
    fn native_selection(&self) -> CharRange {
        let mut selection = CharRange::default();
        self.send(EM_EXGETSEL, 0, (&mut selection as *mut CharRange) as isize);
        selection
    }
    fn set_selection(&self, selection: CharRange) {
        self.send(EM_EXSETSEL, 0, (&selection as *const CharRange) as isize);
    }
    fn empty_char_format(&self) -> CharFormat {
        CharFormat {
            size: size_of::<CharFormat>() as u32,
            ..CharFormat::default()
        }
    }
    fn char_format(&self) -> CharFormat {
        let mut format = self.empty_char_format();
        self.send(
            EM_GETCHARFORMAT,
            SCF_SELECTION,
            (&mut format as *mut CharFormat) as isize,
        );
        format
    }
    fn empty_para_format(&self) -> ParaFormat {
        ParaFormat {
            size: size_of::<ParaFormat>() as u32,
            ..ParaFormat::default()
        }
    }
    fn paragraph_format(&self) -> ParaFormat {
        let mut format = self.empty_para_format();
        self.send(
            EM_GETPARAFORMAT,
            0,
            (&mut format as *mut ParaFormat) as isize,
        );
        format
    }
    fn apply_char(&self, format: CharFormat) -> Result<()> {
        // A plain document has one style for all of its text, including text typed later.
        let scope = if self.state.plain.get() {
            self.send(
                EM_SETCHARFORMAT,
                SCF_DEFAULT,
                (&format as *const CharFormat) as isize,
            );
            SCF_ALL
        } else {
            SCF_SELECTION
        };
        ensure!(
            self.send(
                EM_SETCHARFORMAT,
                scope,
                (&format as *const CharFormat) as isize
            ) != 0,
            "Windows could not apply the character formatting"
        );
        self.mark_modified();
        Ok(())
    }
    fn apply_para(&self, format: ParaFormat) -> Result<()> {
        ensure!(
            self.send(EM_SETPARAFORMAT, 0, (&format as *const ParaFormat) as isize) != 0,
            "Windows could not apply the paragraph formatting"
        );
        self.mark_modified();
        Ok(())
    }
    fn toggle_effect(&self, mask: u32, effect: u32) -> Result<()> {
        let current = self.char_format();
        let mut next = self.empty_char_format();
        next.mask = mask;
        next.effects = if current.mask & mask == mask && current.effects & effect != 0 {
            0
        } else {
            effect
        };
        self.apply_char(next)
    }
    fn transform_case(&self, case: Case) -> Result<()> {
        let original_range = self.selection();
        let selected = self.selected_text()?;
        let (changes, transformed_length) = case_changes(&selected, case, original_range.start);
        if changes.is_empty() {
            return Ok(());
        }
        let _edit = self.begin_collection()?;
        for change in changes.into_iter().rev() {
            self.select_range(change.start..change.start + change.original_len)?;
            let style = self.char_format();
            self.replace_current(&change.replacement)?;
            self.select_range(
                change.start..change.start + change.replacement.encode_utf16().count(),
            )?;
            self.apply_char(style)?;
        }
        self.select_range(original_range.start..original_range.start + transformed_length)
    }
    fn tom_document(&self) -> Result<ITextDocument> {
        let mut ole: *mut std::ffi::c_void = ptr::null_mut();
        ensure!(
            self.send(
                WM_USER + 60,
                0,
                (&mut ole as *mut *mut std::ffi::c_void) as isize
            ) != 0
                && !ole.is_null(),
            "the native text document interface is unavailable"
        );
        // SAFETY: EM_GETOLEINTERFACE returns one owned COM reference.
        unsafe { IUnknown::from_raw(ole).cast() }.context("Obtaining the native text document")
    }
    fn begin_collection(&self) -> Result<EditCollection> {
        let document = self.tom_document()?;
        // SAFETY: the typed COM interface owns this live document reference.
        unsafe {
            document.BeginEditCollection()?;
        }
        Ok(EditCollection(document))
    }
    fn stream_rtf(&self, bytes: &[u8], selection: bool) -> Result<()> {
        let _edit = if selection {
            Some(self.begin_collection()?)
        } else {
            None
        };
        self.read_rtf(bytes, selection)
    }
    // A caller-owned undo collection must span every fragment without nested end calls.
    fn read_rtf(&self, bytes: &[u8], selection: bool) -> Result<()> {
        ensure!(
            !selection || !self.read_only.get(),
            "the document is locked"
        );
        ensure!(
            bytes.len() <= MAX_DOCUMENT_BYTES,
            "the rich-text content is too large"
        );
        let mut input = InputStream { bytes, cursor: 0 };
        let mut stream = EditStream {
            cookie: (&mut input as *mut InputStream<'_>) as usize,
            error: 0,
            callback: stream_in,
        };
        self.send(
            EM_STREAMIN,
            SF_RTF | if selection { SFF_SELECTION } else { 0 },
            (&mut stream as *mut EditStream) as isize,
        );
        let error = stream.error;
        ensure!(
            error == 0,
            "Windows could not read the complete rich-text stream (error {error})"
        );
        if selection {
            self.state.pasted.set(true);
        }
        Ok(())
    }
}
impl Drop for NativeEditor {
    fn drop(&mut self) {
        let data = (&*self.state as *const NativeState) as usize;
        // SAFETY: remove both subclasses before their boxed state drops. The owner
        // hook removes itself on WM_NCDESTROY, so removing it again is a no-op.
        // alive is cleared by WM_NCDESTROY if the owner destroyed the host first.
        unsafe {
            RemoveWindowSubclass(self.state.owner, Some(owner_subclass), data);
            if self.state.alive.get() {
                RemoveWindowSubclass(self.hwnd, Some(editor_subclass), SUBCLASS_ID);
                DestroyWindow(self.state.host);
            }
        }
    }
}
/// Registers the document host window class once per process.
fn host_class() -> Result<Vec<u16>> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let name = wide("ExEditorDocumentHost");
    let registered = *REGISTERED.get_or_init(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            // SAFETY: a null name returns this executable's module handle.
            hInstance: unsafe { GetModuleHandleW(ptr::null()) },
            // SAFETY: stock objects are process-lifetime and need no release.
            hbrBackground: unsafe { GetStockObject(WHITE_BRUSH) },
            lpszClassName: name.as_ptr(),
            ..WNDCLASSW::default()
        };
        // SAFETY: the class structure and its terminated name outlive the call.
        unsafe { RegisterClassW(&class) != 0 }
    });
    ensure!(
        registered,
        "the document host window class could not be registered"
    );
    Ok(name)
}
/// Sizes and focuses the RichEdit child, and mirrors activation to the GPUI owner.
unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    // SAFETY: every call below targets this live host, its child, or its owner on this thread.
    unsafe {
        match message {
            WM_SIZE => {
                let child = GetWindow(hwnd, GW_CHILD);
                if !child.is_null() {
                    let width = (lparam & 0xffff) as i32;
                    let height = ((lparam >> 16) & 0xffff) as i32;
                    MoveWindow(child, 0, 0, width, height, 1);
                }
                0
            }
            windows_sys::Win32::UI::WindowsAndMessaging::WM_COMMAND
                if (wparam >> 16) & 0xffff == EN_CHANGE =>
            {
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    1,
                );
                0
            }
            WM_SETFOCUS | FOCUS_EDITOR => {
                let child = GetWindow(hwnd, GW_CHILD);
                if !child.is_null() {
                    SetFocus(child);
                }
                0
            }
            WM_ACTIVATE => {
                let owner = GetWindow(hwnd, GW_OWNER);
                let other = lparam as HWND;
                // Activation moving between owner and host is internal; anything else
                // is the app gaining or losing focus, which GPUI must still observe.
                if !owner.is_null() && other != owner {
                    let state = if (wparam & 0xffff) as u32 == WA_INACTIVE {
                        WA_INACTIVE as usize
                    } else {
                        // The minimized flag stops DefWindowProc from moving keyboard
                        // focus to the owner; GPUI reads only the low word.
                        WA_ACTIVE as usize | (1 << 16)
                    };
                    SendMessageW(owner, WM_ACTIVATE, state, other as isize);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
/// Keeps the document host over its editor rectangle and the owner visibly active.
unsafe extern "system" fn owner_subclass(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    data: usize,
) -> isize {
    // SAFETY: data is the boxed state that Drop keeps alive until this hook is removed.
    let state = unsafe { &*(data as *const NativeState) };
    match message {
        // The owner losing activation to its own document host is not a real deactivation.
        WM_ACTIVATE if (wparam & 0xffff) as u32 == WA_INACTIVE && lparam as HWND == state.host => {
            return 0;
        }
        WM_WINDOWPOSCHANGED => state.place(),
        WM_NCDESTROY => {
            // SAFETY: remove this callback from its own window during destruction.
            unsafe {
                RemoveWindowSubclass(hwnd, Some(owner_subclass), id);
            }
        }
        _ => {}
    }
    // SAFETY: unhandled messages continue through GPUI's window procedure.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
struct InputStream<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
unsafe extern "system" fn stream_in(
    cookie: usize,
    buffer: *mut u8,
    capacity: i32,
    written: *mut i32,
) -> u32 {
    if cookie == 0 || written.is_null() || capacity < 0 || (capacity > 0 && buffer.is_null()) {
        return 1;
    }
    // SAFETY: cookie points to the exclusive stream for synchronous EM_STREAMIN.
    let input = unsafe { &mut *(cookie as *mut InputStream<'_>) };
    let count = (capacity as usize).min(input.bytes.len().saturating_sub(input.cursor));
    // SAFETY: count fits both the native buffer and the remaining borrowed input.
    unsafe {
        if count > 0 {
            ptr::copy_nonoverlapping(input.bytes.as_ptr().add(input.cursor), buffer, count);
        }
        *written = count as i32;
    }
    input.cursor += count;
    0
}
unsafe extern "system" fn stream_out(
    cookie: usize,
    buffer: *mut u8,
    count: i32,
    written: *mut i32,
) -> u32 {
    if cookie == 0 || written.is_null() || count < 0 || (count > 0 && buffer.is_null()) {
        return 1;
    }
    // SAFETY: cookie is the exclusive output Vec for synchronous EM_STREAMOUT.
    let output = unsafe { &mut *(cookie as *mut Vec<u8>) };
    let count = count as usize;
    if output.len().saturating_add(count) > MAX_DOCUMENT_BYTES || output.try_reserve(count).is_err()
    {
        return 1;
    }
    // SAFETY: native buffer has count readable bytes; output has reserved capacity.
    unsafe {
        if count > 0 {
            ptr::copy_nonoverlapping(buffer, output.as_mut_ptr().add(output.len()), count);
        }
        output.set_len(output.len() + count);
        *written = count as i32;
    }
    0
}

unsafe extern "system" fn editor_subclass(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    data: usize,
) -> isize {
    if message == WM_NCDESTROY {
        if data != 0 {
            // SAFETY: state remains owned until after the window is destroyed.
            unsafe {
                (&*(data as *const NativeState)).alive.set(false);
            }
        }
        // SAFETY: remove this callback from its own native window.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(editor_subclass), id);
        }
    } else if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) && data != 0 {
        // SAFETY: this boxed state and keyboard state belong to this live UI thread.
        let state = unsafe { &*(data as *const NativeState) };
        let modifiers = unsafe {
            crate::keyboard::Modifiers {
                control: GetKeyState(VK_CONTROL as i32) < 0,
                shift: GetKeyState(VK_SHIFT as i32) < 0,
                alt: GetKeyState(VK_MENU as i32) < 0,
            }
        };
        if matches!(wparam, 0x10..=0x12) {
            // Modifier transitions do not cancel a Ctrl+K chord.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let chord = state
            .chord
            .take()
            .is_some_and(|start| start.elapsed() < std::time::Duration::from_secs(3));
        if let Some(action) = crate::keyboard::shortcut(wparam as u32, modifiers, chord) {
            if action == "chord" {
                state.chord.set(Some(std::time::Instant::now()));
            } else {
                state.shortcuts.borrow_mut().push_back(action.into());
            }
            state.consumed_key.set(true);
            return 0;
        }
        state.consumed_key.set(chord);
        if chord {
            return 0;
        }
    } else if matches!(message, WM_CHAR | WM_SYSCHAR) && data != 0 {
        // Suppress only the character generated by a handled shortcut; ordinary typing/IME is native.
        let state = unsafe { &*(data as *const NativeState) };
        if state.consumed_key.replace(false) {
            return 0;
        }
    } else if message == WM_PASTE && data != 0 {
        // SAFETY: data points to boxed state owned until subclass removal.
        let state = unsafe { &*(data as *const NativeState) };
        let (message, wparam) = if state.plain.get() {
            (EM_PASTESPECIAL, CF_UNICODETEXT)
        } else {
            (WM_PASTE, 0)
        };
        // SAFETY: the original procedure handles the paste on this live window.
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, 0) };
        state.pasted.set(true);
        return result;
    } else if message == WM_PAINT && data != 0 {
        // SAFETY: the original procedure paints and validates first; state is owned
        // until subclass removal.
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        let state = unsafe { &*(data as *const NativeState) };
        let drew = crate::emoji::paint(hwnd, state.paper.get());
        state.emoji.set(drew.then(|| crate::emoji::selection(hwnd)));
        return result;
    } else if matches!(
        message,
        WM_KEYUP | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MOUSEMOVE
    ) && data != 0
    {
        // RichEdit redraws a selection change itself; repaint so emoji leaving or
        // entering the selection get the right picture.
        // SAFETY: as above.
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        let state = unsafe { &*(data as *const NativeState) };
        if let Some(painted) = state.emoji.get()
            && crate::emoji::selection(hwnd) != painted
        {
            // SAFETY: invalidates this live window.
            unsafe {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
        }
        return result;
    } else if message == WM_CONTEXTMENU {
        // SAFETY: the helper uses only synchronous calls on this live window.
        unsafe {
            context_menu(hwnd, lparam);
        }
        return 0;
    }
    // SAFETY: unhandled messages continue through the original native procedure.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
/// Maps a Ctrl+key (Windows virtual-key code) to its command name; TextEdit's Cmd becomes Ctrl.
#[cfg(test)]
pub fn shortcut(key: u32, shift: bool) -> Option<&'static str> {
    crate::keyboard::shortcut(
        key,
        crate::keyboard::Modifiers {
            control: true,
            shift,
            alt: false,
        },
        false,
    )
}
unsafe fn context_menu(hwnd: HWND, location: isize) {
    // SAFETY: this menu is used synchronously and destroyed before return.
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    let entries = [
        (1, "Undo"),
        (0, ""),
        (2, "Cut"),
        (3, "Copy"),
        (4, "Paste"),
        (5, "Delete"),
        (0, ""),
        (6, "Select All"),
    ];
    for (id, label) in entries {
        let mut flags = if id == 0 { MF_SEPARATOR } else { MF_STRING };
        // SAFETY: hwnd is the currently subclassed live RichEdit control.
        if id == 1 && unsafe { SendMessageW(hwnd, EM_CANUNDO, 0, 0) } == 0 {
            flags |= MF_GRAYED;
        }
        let label = wide(label);
        // SAFETY: Windows copies the valid terminated label into the menu.
        unsafe {
            AppendMenuW(menu, flags, id, label.as_ptr());
        }
    }
    let mut x = location as i16 as i32;
    let mut y = (location >> 16) as i16 as i32;
    if location == -1 {
        let mut point = windows_sys::Win32::Foundation::POINT::default();
        // SAFETY: point is initialized writable storage for native coordinates.
        unsafe {
            GetCursorPos(&mut point);
        }
        x = point.x;
        y = point.y;
    }
    // SAFETY: live owner, menu and screen coordinates for the synchronous popup.
    let command = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            x,
            y,
            0,
            hwnd,
            ptr::null(),
        )
    };
    // SAFETY: the native popup has returned and no longer retains the menu.
    unsafe {
        DestroyMenu(menu);
    }
    let message = match command {
        1 => WM_UNDO,
        2 => WM_CUT,
        3 => WM_COPY,
        4 => WM_PASTE,
        5 => WM_CLEAR,
        6 => EM_EXSETSEL,
        _ => return,
    };
    if command == 6 {
        let range = CharRange { min: 0, max: -1 };
        // SAFETY: the stack range outlives the synchronous message call.
        unsafe {
            SendMessageW(hwnd, message, 0, (&range as *const CharRange) as isize);
        }
    } else {
        // SAFETY: these standard commands do not dereference their parameters.
        unsafe {
            SendMessageW(hwnd, message, 0, 0);
        }
    }
}
struct PrintResources {
    editor: HWND,
    dialog: PRINTDLGW,
    active: bool,
}
impl Drop for PrintResources {
    fn drop(&mut self) {
        // SAFETY: every returned print handle stays owned until this cleanup;
        // aborting an unfinished job prevents silent partial-success reporting.
        unsafe {
            SendMessageW(self.editor, EM_FORMATRANGE, 0, 0);
            if !self.dialog.hDC.is_null() {
                if self.active {
                    AbortDoc(self.dialog.hDC);
                }
                DeleteDC(self.dialog.hDC);
            }
            if !self.dialog.hDevMode.is_null() {
                GlobalFree(self.dialog.hDevMode);
            }
            if !self.dialog.hDevNames.is_null() {
                GlobalFree(self.dialog.hDevNames);
            }
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn color_ref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
fn validate_page(page: &PageSettings) -> Result<()> {
    ensure!(
        (1..=144_000).contains(&page.width_twips) && (1..=144_000).contains(&page.height_twips),
        "paper dimensions must be positive and no larger than 100 inches"
    );
    ensure!(
        [
            page.margin_left,
            page.margin_right,
            page.margin_top,
            page.margin_bottom
        ]
        .iter()
        .all(|margin| (0..=144_000).contains(margin)),
        "page margins are invalid"
    );
    ensure!(
        page.margin_left + page.margin_right < page.width_twips
            && page.margin_top + page.margin_bottom < page.height_twips,
        "page margins leave no printable content area"
    );
    Ok(())
}
fn rtf_escape(text: &str) -> String {
    let mut result = String::new();
    for unit in text.encode_utf16() {
        match unit {
            0x5c | 0x7b | 0x7d => {
                result.push('\\');
                result.push(char::from_u32(u32::from(unit)).expect("ASCII unit"));
            }
            0x0a => result.push_str("\\line "),
            0x0d => {}
            0x09 => result.push_str("\\tab "),
            0x20..=0x7e => result.push(char::from_u32(u32::from(unit)).expect("ASCII unit")),
            _ => {
                let _ = write!(result, "\\u{}?", unit as i16);
            }
        }
    }
    result
}
#[derive(Debug)]
struct CaseChange {
    start: usize,
    original_len: usize,
    replacement: String,
}
fn case_changes(text: &str, case: Case, start: usize) -> (Vec<CaseChange>, usize) {
    let mut changes = Vec::new();
    let mut offset = start;
    let mut output_len = 0;
    let mut word_start = true;
    for character in text.chars() {
        let replacement: String = match case {
            Case::Upper => character.to_uppercase().collect(),
            Case::Lower => character.to_lowercase().collect(),
            Case::Title if character.is_alphanumeric() && word_start => {
                character.to_uppercase().collect()
            }
            Case::Title if character.is_alphanumeric() => character.to_lowercase().collect(),
            Case::Title => character.to_string(),
        };
        word_start = !character.is_alphanumeric();
        output_len += replacement.encode_utf16().count();
        if replacement != character.to_string() {
            changes.push(CaseChange {
                start: offset,
                original_len: character.len_utf16(),
                replacement,
            });
        }
        offset += character.len_utf16();
    }
    (changes, output_len)
}
fn bitmap_dib(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    ensure!(width > 0 && height > 0, "the image is empty");
    let row = (usize::try_from(width)?
        .checked_mul(3)
        .context("image row is too wide")?
        + 3)
        & !3;
    let body_len = row
        .checked_mul(usize::try_from(height)?)
        .context("image is too large")?;
    ensure!(
        body_len <= MAX_DOCUMENT_BYTES / 2,
        "the decoded image is too large to embed"
    );
    ensure!(
        pixels.len() == usize::try_from(u64::from(width) * u64::from(height) * 4)?,
        "the decoded image has an invalid pixel buffer"
    );
    let mut dib = vec![0_u8; 40 + body_len];
    dib[0..4].copy_from_slice(&40_u32.to_le_bytes());
    dib[4..8].copy_from_slice(&i32::try_from(width)?.to_le_bytes());
    dib[8..12].copy_from_slice(&i32::try_from(height)?.to_le_bytes());
    dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
    dib[14..16].copy_from_slice(&24_u16.to_le_bytes());
    dib[20..24].copy_from_slice(&u32::try_from(body_len)?.to_le_bytes());
    for y in 0..height as usize {
        let target_row = 40 + (height as usize - 1 - y) * row;
        for x in 0..width as usize {
            let source = (y * width as usize + x) * 4;
            let target = target_row + x * 3;
            let alpha = u32::from(pixels[source + 3]);
            for channel in 0..3 {
                let value = u32::from(pixels[source + 2 - channel]);
                dib[target + channel] = ((value * alpha + 255 * (255 - alpha)) / 255) as u8;
            }
        }
    }
    Ok(dib)
}
/// Ends the native undo group on every exit path.
struct EditCollection(ITextDocument);
impl Drop for EditCollection {
    fn drop(&mut self) {
        // SAFETY: BeginEditCollection succeeded; the owned interface stays alive here.
        unsafe {
            let _ = self.0.EndEditCollection();
        }
    }
}
struct QuietEdit<'a> {
    editor: &'a NativeEditor,
    document: ITextDocument,
    selection: CharRange,
    scroll: POINT,
    modified: bool,
}
impl<'a> QuietEdit<'a> {
    fn new(editor: &'a NativeEditor) -> Result<Self> {
        let document = editor.tom_document()?;
        let mut scroll = POINT::default();
        editor.send(EM_GETSCROLLPOS, 0, (&mut scroll as *mut POINT) as isize);
        // SAFETY: the typed interface owns the live UI-thread document.
        unsafe {
            document.Freeze()?;
            if let Err(error) = document.Undo(TOM_SUSPEND) {
                let _ = document.Unfreeze();
                return Err(error.into());
            }
        }
        let quiet = Self {
            selection: editor.native_selection(),
            modified: editor.is_modified(),
            scroll,
            editor,
            document,
        };
        if editor.read_only.get() {
            editor.send(EM_SETREADONLY, 0, 0);
        }
        Ok(quiet)
    }
}
impl Drop for QuietEdit<'_> {
    fn drop(&mut self) {
        let editor = self.editor;
        editor.set_selection(self.selection);
        if editor.read_only.get() {
            editor.send(EM_SETREADONLY, 1, 0);
        }
        editor.send(EM_SETMODIFY, usize::from(self.modified), 0);
        // SAFETY: this guard successfully froze the document and suspended undo.
        unsafe {
            let _ = self.document.Undo(TOM_RESUME);
            let _ = self.document.Unfreeze();
        }
        editor.send(EM_SETSCROLLPOS, 0, (&self.scroll as *const POINT) as isize);
    }
}
/// Printing must ignore display-only colors, and restore them even on an error.
struct PrintAppearance<'a> {
    editor: &'a NativeEditor,
    previous: usize,
}
impl<'a> PrintAppearance<'a> {
    fn new(editor: &'a NativeEditor) -> Self {
        const HIDE_TEMPORARY: usize = 0x1000_0000;
        let previous = editor.send(WM_USER + 276, 0, 0) as usize;
        editor.send(WM_USER + 275, HIDE_TEMPORARY, HIDE_TEMPORARY as isize);
        Self { editor, previous }
    }
}
impl Drop for PrintAppearance<'_> {
    fn drop(&mut self) {
        const HIDE_TEMPORARY: usize = 0x1000_0000;
        self.editor.send(
            WM_USER + 275,
            self.previous & HIDE_TEMPORARY,
            HIDE_TEMPORARY as isize,
        );
    }
}

fn printer_mode(page: &PageSettings) -> Result<windows_sys::Win32::Graphics::Gdi::DEVMODEW> {
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, DEVMODEW_0, DEVMODEW_0_0, DM_ORIENTATION, DM_PAPERLENGTH, DM_PAPERSIZE,
        DM_PAPERWIDTH,
    };
    let portrait_width = page.width_twips.min(page.height_twips);
    let portrait_height = page.width_twips.max(page.height_twips);
    let paper_size = match (portrait_width, portrait_height) {
        (12_240, 15_840) => 1,
        (11_906..=11_908, 16_837..=16_839) => 9,
        _ => 0,
    };
    let fields = DM_ORIENTATION
        | if paper_size != 0 {
            DM_PAPERSIZE
        } else {
            DM_PAPERWIDTH | DM_PAPERLENGTH
        };
    Ok(DEVMODEW {
        dmSpecVersion: 0x0401,
        dmSize: size_of::<DEVMODEW>() as u16,
        dmFields: fields,
        Anonymous1: DEVMODEW_0 {
            Anonymous1: DEVMODEW_0_0 {
                dmOrientation: if page.width_twips > page.height_twips {
                    2
                } else {
                    1
                },
                dmPaperSize: paper_size,
                dmPaperWidth: i16::try_from((i64::from(portrait_width) * 254 + 720) / 1440)?,
                dmPaperLength: i16::try_from((i64::from(portrait_height) * 254 + 720) / 1440)?,
                ..DEVMODEW_0_0::default()
            },
        },
        ..DEVMODEW::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    struct TestOwner(HWND);
    impl Drop for TestOwner {
        fn drop(&mut self) {
            // SAFETY: the native editor is dropped before its private, hidden owner.
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    struct TestEditor {
        editor: NativeEditor,
        _owner: TestOwner,
    }
    fn hidden_editor() -> TestEditor {
        let class = wide("STATIC");
        let title = wide("");
        // SAFETY: creates a hidden, system-class test owner on this thread.
        let owner = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                400,
                300,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!owner.is_null(), "hidden test owner");
        let owner = TestOwner(owner);
        let editor = NativeEditor::new(owner.0 as isize).expect("hidden native editor");
        TestEditor {
            editor,
            _owner: owner,
        }
    }
    #[test]
    fn display_colors_do_not_change_rtf_or_explicit_color_identity() {
        let test = hidden_editor();
        let editor = &test.editor;
        editor
            .set_rtf(
                br"{\rtf1\ansi{\colortbl;\red230\green230\blue229;}\cf0 automatic \cf1 explicit}",
            )
            .expect("fixture");
        editor.mark_saved();
        let before = editor.rtf().expect("canonical RTF");
        editor.set_background(0x1e1e1e);
        editor
            .set_text_override(Some(0xe6e6e5))
            .expect("display-only override");
        assert_eq!(
            editor.rtf().expect("dark RTF"),
            before,
            "dark mode must never rewrite canonical colors"
        );
        assert!(!editor.is_modified(), "appearance is not an edit");
        editor
            .set_text_override(None)
            .expect("remove display override");
        assert_eq!(editor.rtf().expect("restored RTF"), before);
    }
    #[test]
    fn preview_leaves_source_undo_selection_and_unsaved_edits_intact() {
        let test = hidden_editor();
        let editor = &test.editor;
        editor.set_text("source").expect("source");
        editor.mark_saved();
        editor.select_range(6..6).expect("caret");
        editor.replace_current(" changed").expect("edit");
        editor.select_range(1..4).expect("selection");
        let preview = editor
            .preview(br"{\rtf1 preview}")
            .expect("independent preview");
        assert_eq!(editor.text().expect("source text"), "source changed");
        assert_eq!(editor.selection(), 1..4);
        assert!(editor.is_modified(), "pending edit remains pending");
        drop(preview);
        editor.execute(EditCommand::Undo).expect("source undo");
        assert_eq!(editor.text().expect("undone source"), "source");
    }
    #[test]
    fn rich_line_move_preserves_bold_and_is_one_undo_step() {
        let test = hidden_editor();
        let editor = &test.editor;
        editor
            .set_rtf(br"{\rtf1\ansi\b alpha\b0\par beta}")
            .expect("mixed styles");
        let before = editor.text().expect("text");
        let edit = crate::editing::plan(&before, 0..5, crate::editing::LineAction::MoveDown, "//")
            .expect("plan")
            .expect("move");
        let selected = edit.selection.clone();
        editor.edit_lines(edit).expect("rich move");
        assert_eq!(editor.text().expect("moved"), "beta\nalpha");
        editor
            .select_range(selected.start..selected.start + 1)
            .expect("moved run");
        assert!(editor.formatting().bold, "moved run keeps its style");
        editor.execute(EditCommand::Undo).expect("one undo");
        assert_eq!(editor.text().expect("restored"), before);
        editor.select_range(0..1).expect("original run");
        assert!(editor.formatting().bold, "undo restores the original style");
        editor.execute(EditCommand::Redo).expect("one redo");
        assert_eq!(editor.text().expect("redone"), "beta\nalpha");
    }
    #[test]
    fn print_temporary_color_mask_is_restored_on_early_exit() {
        let test = hidden_editor();
        let before = test.editor.send(WM_USER + 276, 0, 0);
        {
            let _print = PrintAppearance::new(&test.editor);
            assert_ne!(test.editor.send(WM_USER + 276, 0, 0) & 0x1000_0000, 0);
        }
        assert_eq!(test.editor.send(WM_USER + 276, 0, 0), before);
    }
    #[test]
    fn document_host_tracks_owner_client_rectangle() {
        let class = wide("STATIC");
        let empty = wide("");
        // SAFETY: system class and terminated strings; the window is destroyed below.
        let owner = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                empty.as_ptr(),
                WS_POPUP,
                100,
                100,
                400,
                300,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!owner.is_null(), "test owner window was not created");
        let editor = NativeEditor::new(owner as isize).expect("editor hosts inside its owner");
        editor.set_bounds(10, 20, 200, 100);
        let mut host = RECT::default();
        let mut child = RECT::default();
        // SAFETY: live windows on this thread with writable rectangles.
        unsafe {
            GetWindowRect(editor.state.host, &mut host);
            GetWindowRect(editor.hwnd, &mut child);
        }
        assert_eq!(
            (host.left, host.top, host.right, host.bottom),
            (110, 120, 310, 220)
        );
        assert_eq!(
            (child.left, child.top, child.right, child.bottom),
            (110, 120, 310, 220)
        );
        drop(editor);
        // SAFETY: the owner was created above and has not been destroyed.
        unsafe {
            DestroyWindow(owner);
        }
    }
    #[test]
    fn rich_edit_abi_matches_microsoft_pack_four_layout() {
        assert_eq!(size_of::<CharFormat>(), 116);
        assert_eq!(size_of::<ParaFormat>(), 188);
        assert_eq!(size_of::<EditStream>(), size_of::<usize>() * 2 + 4);
    }
    #[test]
    fn rtf_escape_preserves_unicode_and_literal_controls() {
        assert_eq!(
            rtf_escape("a{\\}\n😀"),
            "a\\{\\\\\\}\\line \\u-10179?\\u-8704?"
        );
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }
    #[test]
    fn dib_rows_are_bottom_up_bgr_and_padded() {
        let dib = bitmap_dib(1, 2, &[255, 0, 0, 255, 0, 0, 255, 255]).expect("valid pixels");
        assert_eq!(&dib[40..], &[255, 0, 0, 0, 0, 0, 255, 0]);
    }
    #[test]
    fn title_case_and_application_shortcuts_are_explicit() {
        let (changes, length) = case_changes("😀aß", Case::Upper, 10);
        assert_eq!(length, 5);
        assert_eq!(changes[0].start, 12);
        assert_eq!(changes[0].replacement, "A");
        assert_eq!(changes[1].start, 13);
        assert_eq!(changes[1].original_len, 1);
        assert_eq!(changes[1].replacement, "SS");
        assert_eq!(shortcut(0x53, false), Some("save"));
        assert_eq!(shortcut(0x53, true), Some("save-as"));
        assert_eq!(shortcut(0x42, false), Some("bold"));
        assert_eq!(shortcut(0x5a, false), Some("undo"));
        assert_eq!(shortcut(0x5a, true), Some("redo"));
        assert_eq!(shortcut(0x56, false), Some("paste"));
        assert_eq!(shortcut(0xbc, false), Some("preferences"));
        assert_eq!(shortcut(0xbc, true), Some("zoom-out"));
        assert_eq!(shortcut(0xbb, true), Some("bigger"));
        // Ctrl+I's control character (Tab) is consumed, not typed.
        assert_eq!(shortcut(9 + 0x40, false), Some("italic"));
    }
}
