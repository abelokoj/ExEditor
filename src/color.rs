//! CSS color values, persisted swatches, and the user-initiated screen sampler.
use crate::document::{atomic_write, data_dir};
use anyhow::{Context as _, Result, ensure};
use csscolorparser::Color;
use serde::{Deserialize, Serialize};
use std::{fs, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Rgba {
    pub const fn from_rgb(rgb: u32) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a: 255,
        }
    }
    pub const fn rgb(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }
    pub fn hex(self, alpha: bool, uppercase: bool) -> String {
        let mut code = if alpha {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        } else {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        };
        if uppercase {
            code.make_ascii_uppercase();
        }
        code
    }
    /// Hue is measured in degrees; saturation and value are normalized to 0..1.
    pub fn hsv(self) -> (f32, f32, f32) {
        let [h, s, v, _] = self.css().to_hsva();
        (h, s, v)
    }
    pub fn from_hsv(hue: f32, saturation: f32, value: f32, alpha: u8) -> Self {
        let finite = |value: f32| if value.is_finite() { value } else { 0. };
        let [r, g, b, a] = Color::from_hsva(
            finite(hue).rem_euclid(360.),
            finite(saturation).clamp(0., 1.),
            finite(value).clamp(0., 1.),
            f32::from(alpha) / 255.,
        )
        .to_rgba8();
        Self { r, g, b, a }
    }
    /// Composites onto an opaque RGB background using ordinary sRGB channel values.
    pub fn composite(self, background: u32) -> u32 {
        let background = Self::from_rgb(background);
        let channel = |foreground: u8, background: u8| {
            (u32::from(foreground) * u32::from(self.a)
                + u32::from(background) * (255 - u32::from(self.a))
                + 127)
                / 255
        };
        (channel(self.r, background.r) << 16)
            | (channel(self.g, background.g) << 8)
            | channel(self.b, background.b)
    }
    pub fn parse(value: &str) -> Result<(Self, ColorFormat)> {
        let value = value.trim();
        ensure!(
            !value.is_empty(),
            "Enter a color code or stylesheet color name."
        );
        ensure!(
            value.len() <= 4096 && !value.contains('\0'),
            "The color code is too long or contains a NUL character."
        );
        let parsed: Color = value.parse().context(
            "Enter a valid CSS color, such as #80bf40, rgb(128, 191, 64), or rebeccapurple",
        )?;
        let [r, g, b, a] = parsed.clamp().to_rgba8();
        let color = Self { r, g, b, a };
        let lower = value.to_ascii_lowercase();
        let bare = lower.strip_prefix('#').unwrap_or(&lower);
        let hex = bare.bytes().all(|byte| byte.is_ascii_hexdigit());
        let format = if hex {
            match bare.len() {
                3 => ColorFormat::ShortHex,
                4 => ColorFormat::ShortHexAlpha,
                6 => ColorFormat::Hex,
                8 => ColorFormat::HexAlpha,
                _ => ColorFormat::Hex,
            }
        } else if lower == "transparent" || named_colors().iter().any(|(name, _)| *name == lower) {
            ColorFormat::Keyword
        } else if lower.starts_with("rgba(") || (lower.starts_with("rgb(") && lower.contains('/')) {
            ColorFormat::Rgba
        } else if lower.starts_with("rgb(") {
            ColorFormat::Rgb
        } else if lower.starts_with("hsla(") || (lower.starts_with("hsl(") && lower.contains('/')) {
            ColorFormat::Hsla
        } else if lower.starts_with("hsl(") {
            ColorFormat::Hsl
        } else if lower.starts_with("hwba(") || (lower.starts_with("hwb(") && lower.contains('/')) {
            ColorFormat::Hwba
        } else if lower.starts_with("hwb(") {
            ColorFormat::Hwb
        } else if color.a == 255 {
            ColorFormat::Hex
        } else {
            ColorFormat::HexAlpha
        };
        Ok((color, format))
    }
    fn css(self) -> Color {
        Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorFormat {
    #[default]
    Hex,
    HexAlpha,
    ShortHex,
    ShortHexAlpha,
    Rgb,
    Rgba,
    Hsl,
    Hsla,
    Hwb,
    Hwba,
    Keyword,
}
impl ColorFormat {
    pub const ALL: [Self; 11] = [
        Self::Hex,
        Self::HexAlpha,
        Self::ShortHex,
        Self::ShortHexAlpha,
        Self::Rgb,
        Self::Rgba,
        Self::Hsl,
        Self::Hsla,
        Self::Hwb,
        Self::Hwba,
        Self::Keyword,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hex => "Hexadecimal",
            Self::HexAlpha => "Hexadecimal with Alpha",
            Self::ShortHex => "Short Hexadecimal",
            Self::ShortHexAlpha => "Short Hexadecimal with Alpha",
            Self::Rgb => "RGB",
            Self::Rgba => "RGB with Alpha",
            Self::Hsl => "HSL",
            Self::Hsla => "HSL with Alpha",
            Self::Hwb => "HWB",
            Self::Hwba => "HWB with Alpha",
            Self::Keyword => "CSS Keyword",
        }
    }
    pub fn format(self, color: Rgba, uppercase: bool) -> String {
        let alpha = decimal(f32::from(color.a) / 255., 3);
        match self {
            Self::Hex => color.hex(false, uppercase),
            Self::HexAlpha => color.hex(true, uppercase),
            Self::ShortHex | Self::ShortHexAlpha => {
                let with_alpha = self == Self::ShortHexAlpha;
                let channels = [color.r, color.g, color.b, color.a];
                let count = if with_alpha { 4 } else { 3 };
                if channels[..count]
                    .iter()
                    .all(|channel| channel >> 4 == channel & 0x0f)
                {
                    let mut code = String::from("#");
                    for channel in &channels[..count] {
                        use std::fmt::Write as _;
                        let _ = write!(code, "{:x}", channel & 0x0f);
                    }
                    if uppercase {
                        code.make_ascii_uppercase();
                    }
                    code
                } else {
                    // A short hex spelling cannot represent every color exactly.
                    color.hex(with_alpha, uppercase)
                }
            }
            Self::Rgb => format!("rgb({}, {}, {})", color.r, color.g, color.b),
            Self::Rgba => format!("rgba({}, {}, {}, {alpha})", color.r, color.g, color.b),
            Self::Hsl | Self::Hsla => {
                let [h, s, l, _] = color.css().to_hsla();
                let (h, s, l) = (decimal(h, 3), decimal(s * 100., 3), decimal(l * 100., 3));
                if self == Self::Hsla {
                    format!("hsla({h}, {s}%, {l}%, {alpha})")
                } else {
                    format!("hsl({h}, {s}%, {l}%)")
                }
            }
            Self::Hwb | Self::Hwba => {
                let [h, w, b, _] = color.css().to_hwba();
                let (h, w, b) = (decimal(h, 3), decimal(w * 100., 3), decimal(b * 100., 3));
                if self == Self::Hwba {
                    format!("hwb({h} {w}% {b}% / {alpha})")
                } else {
                    format!("hwb({h} {w}% {b}%)")
                }
            }
            Self::Keyword => {
                if color.a == 0 {
                    "transparent".into()
                } else if color.a == 255 {
                    named_colors()
                        .iter()
                        .find(|(_, value)| *value == color.rgb())
                        .map_or_else(
                            || color.hex(false, uppercase),
                            |(name, _)| (*name).to_owned(),
                        )
                } else {
                    color.hex(true, uppercase)
                }
            }
        }
    }
}
fn decimal(value: f32, places: usize) -> String {
    let value = if value.abs() < 0.000_000_1 { 0. } else { value };
    let text = format!("{value:.places$}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// All 148 CSS named colors from csscolorparser's W3C-backed named-color table.
pub fn named_colors() -> &'static [(&'static str, u32)] {
    static COLORS: OnceLock<Vec<(&'static str, u32)>> = OnceLock::new();
    COLORS.get_or_init(|| {
        let mut colors: Vec<_> = csscolorparser::NAMED_COLORS
            .entries()
            .map(|(name, [r, g, b])| {
                (
                    name.as_str(),
                    (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b),
                )
            })
            .collect();
        colors.sort_unstable_by_key(|(name, _)| *name);
        colors
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    Foreground,
    Background,
    InsertCode,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PickerState {
    pub swatches: Vec<Rgba>,
    pub format: ColorFormat,
    pub last_color: Rgba,
}
impl Default for PickerState {
    fn default() -> Self {
        let mut swatches = vec![Rgba::from_rgb(0xffffff); 24];
        swatches[0] = Rgba::from_rgb(0x2589a4);
        swatches[1] = Rgba::from_rgb(0x80bf40);
        Self {
            swatches,
            format: ColorFormat::Hex,
            last_color: Rgba::from_rgb(0x80bf40),
        }
    }
}
impl PickerState {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join("colors.json");
        if !path.try_exists()? {
            return Ok(Self::default());
        }
        ensure!(
            fs::metadata(&path)?.len() <= 64 * 1024,
            "The saved color palette is unexpectedly large; it has not been changed."
        );
        let mut state: Self =
            serde_json::from_slice(&fs::read(&path).context("Reading saved colors")?)
                .context("Reading saved colors; the original palette has not been changed")?;
        ensure!(
            state.swatches.len() <= 256,
            "The saved color palette contains too many swatches."
        );
        state.swatches.resize(24, Rgba::from_rgb(0xffffff));
        state.swatches.truncate(24);
        Ok(state)
    }
    pub fn save(&self) -> Result<()> {
        ensure!(
            self.swatches.len() == 24,
            "The color palette must contain 24 swatches."
        );
        atomic_write(
            &data_dir()?.join("colors.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

/// Samples a desktop pixel using a temporary native overlay that consumes the
/// selection click. The screenshot stays in GDI memory and is never written out.
pub fn pick_screen_color() -> Result<Option<Rgba>> {
    screen_sampler::pick()
}

#[cfg(windows)]
mod screen_sampler {
    use super::Rgba;
    use anyhow::{Result, ensure};
    use std::{cell::Cell, ptr, sync::OnceLock};
    use windows_sys::Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
        },
        Graphics::Gdi::{
            BeginPaint, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC,
            DeleteObject, EndPaint, GetDC, GetPixel, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT, ReleaseDC,
            SRCCOPY, SelectObject,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture, SetFocus},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetForegroundWindow,
                GetMessageW, GetSystemMetrics, GetWindowLongPtrW, IDC_CROSS, IsWindow, LoadCursorW,
                MSG, PostQuitMessage, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, SetForegroundWindow,
                SetWindowLongPtrW, ShowWindow, TranslateMessage, WA_INACTIVE, WM_ACTIVATE,
                WM_CANCELMODE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    };

    struct Capture {
        screen: HDC,
        memory: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
    }
    impl Capture {
        fn new(x: i32, y: i32, width: i32, height: i32) -> Result<Self> {
            // SAFETY: a null HWND requests the virtual desktop's display DC.
            let screen = unsafe { GetDC(ptr::null_mut()) };
            ensure!(
                !screen.is_null(),
                "Windows could not read the desktop display."
            );
            let mut capture = Self {
                screen,
                memory: ptr::null_mut(),
                bitmap: ptr::null_mut(),
                previous: ptr::null_mut(),
            };
            // SAFETY: screen is a live display DC owned until Capture drops.
            capture.memory = unsafe { CreateCompatibleDC(screen) };
            ensure!(
                !capture.memory.is_null(),
                "Windows could not create the sampling buffer."
            );
            // SAFETY: validated positive dimensions and a compatible live DC.
            capture.bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
            ensure!(
                !capture.bitmap.is_null(),
                "Windows could not allocate the desktop sampling image."
            );
            // SAFETY: bitmap is compatible with memory and remains owned by capture.
            capture.previous = unsafe { SelectObject(capture.memory, capture.bitmap as HGDIOBJ) };
            ensure!(
                !capture.previous.is_null() && capture.previous as isize != -1,
                "Windows could not prepare the sampling image."
            );
            // SAFETY: source and destination DCs are live, dimensions match the
            // owned bitmap, and coordinates reference the virtual desktop.
            ensure!(
                unsafe {
                    BitBlt(
                        capture.memory,
                        0,
                        0,
                        width,
                        height,
                        screen,
                        x,
                        y,
                        SRCCOPY | CAPTUREBLT,
                    )
                } != 0,
                "Windows could not capture the desktop for color sampling."
            );
            Ok(capture)
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            // SAFETY: these resources were created above. Restore the originally
            // selected bitmap before deleting the owned bitmap and its memory DC.
            unsafe {
                if !self.memory.is_null()
                    && !self.previous.is_null()
                    && self.previous as isize != -1
                {
                    SelectObject(self.memory, self.previous);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap as HGDIOBJ);
                }
                if !self.memory.is_null() {
                    DeleteDC(self.memory);
                }
                if !self.screen.is_null() {
                    ReleaseDC(ptr::null_mut(), self.screen);
                }
            }
        }
    }
    struct State {
        capture: Capture,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        done: Cell<bool>,
        alive: Cell<bool>,
        failed: Cell<bool>,
        selected: Cell<Option<Rgba>>,
    }
    struct Overlay<'a> {
        hwnd: HWND,
        alive: &'a Cell<bool>,
    }
    impl Drop for Overlay<'_> {
        fn drop(&mut self) {
            // SAFETY: this guard belongs to the created overlay and runs before
            // State drops. The alive flag excludes any subsequently reused HWND value.
            unsafe {
                if self.alive.get() {
                    DestroyWindow(self.hwnd);
                }
            }
        }
    }

    pub fn pick() -> Result<Option<Rgba>> {
        // SAFETY: virtual desktop metrics are process-global read-only values.
        let (x, y, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        ensure!(
            width > 0 && height > 0 && i64::from(width) * i64::from(height) <= 64 * 1024 * 1024,
            "The desktop dimensions are unavailable or exceed the color sampler's memory limit."
        );
        let capture = Capture::new(x, y, width, height)?;
        let mut state = Box::new(State {
            capture,
            x,
            y,
            width,
            height,
            done: Cell::new(false),
            alive: Cell::new(false),
            failed: Cell::new(false),
            selected: Cell::new(None),
        });
        let class: Vec<u16> = "ExEditor.ScreenColorSampler\0".encode_utf16().collect();
        // SAFETY: a null module name requests the process executable's instance.
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        ensure!(
            !instance.is_null(),
            "Windows could not identify the color sampler's module."
        );
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let registered = *REGISTERED.get_or_init(|| {
            let definition = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                // SAFETY: IDC_CROSS is a system cursor identifier; ownership stays with Windows.
                hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_CROSS) },
                ..WNDCLASSW::default()
            };
            // SAFETY: all class pointers remain valid throughout this registration;
            // Windows copies the class name and retains only the static procedure.
            unsafe {
                RegisterClassW(&definition) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS
            }
        });
        ensure!(
            registered,
            "Windows could not register the desktop color sampler."
        );
        let title: Vec<u16> = "Pick a color — click or press Enter; Escape cancels\0"
            .encode_utf16()
            .collect();
        // SAFETY: getting the foreground HWND does not transfer ownership.
        let previous_window = unsafe { GetForegroundWindow() };
        // SAFETY: state is boxed at a stable address until after Overlay drops;
        // class, title, instance, and positive dimensions have been validated.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                x,
                y,
                width,
                height,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                (&mut *state as *mut State).cast(),
            )
        };
        ensure!(
            !hwnd.is_null(),
            "Windows could not open the desktop color sampler: {}",
            std::io::Error::last_os_error()
        );
        let overlay = Overlay {
            hwnd,
            alive: &state.alive,
        };
        // SAFETY: this visible foreground overlay owns the input until the
        // selection or cancellation path destroys it. The click cannot leak.
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            SetCapture(hwnd);
        }
        let mut quit_code = None;
        let mut pump_error = false;
        while !state.done.get() {
            let mut message = MSG::default();
            // SAFETY: writable MSG storage and a thread-local native message pump.
            let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            if result == -1 {
                pump_error = true;
                break;
            }
            if result == 0 {
                quit_code = Some(message.wParam);
                break;
            }
            // SAFETY: GetMessageW supplied an initialized message for this thread.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        drop(overlay);
        // SAFETY: restoring foreground is best-effort and only targets a still
        // valid pre-existing window. Repost WM_QUIT for the application's outer loop.
        unsafe {
            if !previous_window.is_null() && IsWindow(previous_window) != 0 {
                SetForegroundWindow(previous_window);
            }
            if let Some(code) = quit_code {
                PostQuitMessage(code as i32);
            }
        }
        ensure!(
            !pump_error,
            "The desktop color sampler's message loop failed."
        );
        ensure!(
            !state.failed.get(),
            "Windows could not read the chosen desktop pixel."
        );
        Ok(state.selected.get())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            // SAFETY: Windows provides CREATESTRUCTW for WM_NCCREATE and its
            // user parameter is our stable boxed State pointer from CreateWindowExW.
            let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
            let state = create.lpCreateParams as *mut State;
            if state.is_null() {
                return 0;
            }
            // SAFETY: initialization precedes any use of the pointer by this procedure.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                (*state).alive.set(true);
            }
        }
        // SAFETY: reads only this overlay's application-owned user-data slot.
        let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
        if state_ptr.is_null() {
            // SAFETY: standard default processing for messages before initialization.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        // SAFETY: State lives until after Overlay has destroyed this native window.
        let state = unsafe { &*state_ptr };
        match message {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                // SAFETY: BeginPaint initializes the PAINTSTRUCT for this live window.
                let dc = unsafe { BeginPaint(hwnd, &mut paint) };
                if !dc.is_null() {
                    // SAFETY: capture owns a compatible bitmap covering the whole window.
                    if unsafe {
                        BitBlt(
                            dc,
                            0,
                            0,
                            state.width,
                            state.height,
                            state.capture.memory,
                            0,
                            0,
                            SRCCOPY,
                        )
                    } == 0
                    {
                        state.failed.set(true);
                    }
                } else {
                    state.failed.set(true);
                }
                // SAFETY: every BeginPaint is paired with EndPaint on the same window.
                unsafe {
                    EndPaint(hwnd, &paint);
                }
                return 0;
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => return 0,
            WM_LBUTTONUP => {
                // SAFETY: sampling only reads this overlay's owned captured bitmap.
                unsafe {
                    select_pixel(state);
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_KEYDOWN if wparam == 0x0d => {
                // SAFETY: keyboard selection uses the same checked cursor coordinates.
                unsafe {
                    select_pixel(state);
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_KEYDOWN if wparam == 0x1b => {
                // SAFETY: cancelling destroys only this owned overlay.
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_RBUTTONUP | WM_CANCELMODE | WM_CLOSE => {
                // SAFETY: cancellation owns this overlay, not any other application.
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_ACTIVATE if (wparam & 0xffff) == WA_INACTIVE as usize => {
                // SAFETY: losing activation cancels input capture rather than trapping it.
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_DESTROY => {
                state.done.set(true);
                // SAFETY: only release mouse capture if this overlay still owns it.
                unsafe {
                    if GetCapture() == hwnd {
                        ReleaseCapture();
                    }
                }
                return 0;
            }
            WM_NCDESTROY => {
                state.alive.set(false);
                state.done.set(true);
                // SAFETY: no future message may dereference the state after teardown.
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            _ => {}
        }
        // SAFETY: Windows handles all remaining standard window messages.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
    unsafe fn select_pixel(state: &State) {
        let mut point = POINT::default();
        // SAFETY: point is initialized writable native POINT storage.
        if unsafe { GetCursorPos(&mut point) } == 0 {
            state.failed.set(true);
            return;
        }
        let x = point.x.saturating_sub(state.x);
        let y = point.y.saturating_sub(state.y);
        if x < 0 || y < 0 || x >= state.width || y >= state.height {
            return;
        }
        // SAFETY: coordinates were checked against the captured bitmap dimensions.
        let native = unsafe { GetPixel(state.capture.memory, x, y) };
        if native == u32::MAX {
            state.failed.set(true);
            return;
        }
        state.selected.set(Some(Rgba {
            r: native as u8,
            g: (native >> 8) as u8,
            b: (native >> 16) as u8,
            a: 255,
        }));
    }
}
#[cfg(not(windows))]
mod screen_sampler {
    pub fn pick() -> anyhow::Result<Option<super::Rgba>> {
        anyhow::bail!("Desktop color sampling is available in the Windows application.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_css_syntax_and_detects_the_matching_format() {
        for (source, expected, format) in [
            ("#80BF40", Rgba::from_rgb(0x80bf40), ColorFormat::Hex),
            ("#abc", Rgba::from_rgb(0xaabbcc), ColorFormat::ShortHex),
            (
                "#abcd",
                Rgba {
                    r: 0xaa,
                    g: 0xbb,
                    b: 0xcc,
                    a: 0xdd,
                },
                ColorFormat::ShortHexAlpha,
            ),
            (
                "rgba(255, 0, 0, 0.5)",
                Rgba {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 128,
                },
                ColorFormat::Rgba,
            ),
            (
                "rgb(100% 0% 0% / 50%)",
                Rgba {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 128,
                },
                ColorFormat::Rgba,
            ),
            (
                "hsl(120, 100%, 50%)",
                Rgba::from_rgb(0x00ff00),
                ColorFormat::Hsl,
            ),
            (
                "hwb(240 0% 0% / 50%)",
                Rgba {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 128,
                },
                ColorFormat::Hwba,
            ),
            (
                "rebeccapurple",
                Rgba::from_rgb(0x663399),
                ColorFormat::Keyword,
            ),
            ("transparent", Rgba::default(), ColorFormat::Keyword),
        ] {
            assert_eq!(
                Rgba::parse(source).expect(source),
                (expected, format),
                "source {source}"
            );
        }
    }
    #[test]
    fn color_formats_round_trip_without_changing_supported_channels() {
        let color = Rgba {
            r: 128,
            g: 191,
            b: 64,
            a: 127,
        };
        for format in ColorFormat::ALL {
            let code = format.format(color, false);
            let actual = Rgba::parse(&code).expect(&code).0;
            let retains_alpha = matches!(
                format,
                ColorFormat::HexAlpha
                    | ColorFormat::ShortHexAlpha
                    | ColorFormat::Rgba
                    | ColorFormat::Hsla
                    | ColorFormat::Hwba
                    | ColorFormat::Keyword
            );
            assert_eq!(
                actual,
                Rgba {
                    a: if retains_alpha { color.a } else { 255 },
                    ..color
                },
                "format {format:?}: {code}"
            );
        }
    }
    #[test]
    fn short_hex_falls_back_to_exact_long_hex_when_needed() {
        assert_eq!(
            ColorFormat::ShortHex.format(Rgba::from_rgb(0xaabbcc), true),
            "#ABC"
        );
        assert_eq!(
            ColorFormat::ShortHex.format(Rgba::from_rgb(0x80bf40), false),
            "#80bf40"
        );
        assert_eq!(
            ColorFormat::Keyword.format(Rgba::from_rgb(0xff0000), false),
            "red"
        );
        assert_eq!(
            ColorFormat::Keyword.format(Rgba::from_rgb(0x123456), false),
            "#123456"
        );
    }
    #[test]
    fn hsv_uses_degrees_and_compositing_respects_alpha() {
        assert_eq!(Rgba::from_hsv(120., 1., 1., 255), Rgba::from_rgb(0x00ff00));
        let (hue, saturation, value) = Rgba::from_rgb(0x0000ff).hsv();
        assert!((hue - 240.).abs() < 0.001 && saturation == 1. && value == 1.);
        assert_eq!(
            Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 128
            }
            .composite(0xffffff),
            0xff7f7f
        );
        assert_eq!(
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 0
            }
            .composite(0x123456),
            0x123456
        );
    }
    #[test]
    fn all_named_colors_and_saved_palette_serialize_cleanly() {
        assert!(named_colors().len() >= 148);
        for (name, rgb) in named_colors() {
            assert_eq!(Rgba::parse(name).expect(name).0.rgb(), *rgb, "name {name}");
        }
        let state = PickerState::default();
        let decoded: PickerState =
            serde_json::from_str(&serde_json::to_string(&state).expect("serialize colors"))
                .expect("deserialize colors");
        assert_eq!(decoded.swatches, state.swatches);
        assert_eq!(decoded.swatches.len(), 24);
        assert_eq!(decoded.format, state.format);
    }
    #[test]
    fn parser_rejects_invalid_and_oversized_untrusted_input() {
        for source in ["", "#12", "rgb(nope)", "not-a-css-color", "red\0"] {
            assert!(Rgba::parse(source).is_err(), "source {source:?}");
        }
        assert!(Rgba::parse(&"a".repeat(4097)).is_err());
    }
}
