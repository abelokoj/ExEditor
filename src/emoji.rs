//! Color emoji. Windows' windowed RichEdit draws text through GDI, which has no color
//! glyphs, so after each paint the visible emoji are drawn again with DirectWrite's color
//! fonts, the way phones and browsers show them. Skin tones, flags, keycaps and joined
//! sequences (families, professions) become one picture.
use std::{cell::RefCell, ffi::c_void, ops::Range, ptr};
use unicode_segmentation::UnicodeSegmentation as _;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{
                Common::{D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_CLIP,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
                ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
        UI::Controls::RichEdit::{
            ITextDocument, ITextPara2, ITextRange, tomAutoColor, tomConstants, tomParaEffectTable,
            tomParaEffectTableRowDelimiter,
        },
    },
    core::{IUnknown, Interface, w},
};
use windows_sys::Win32::{
    Foundation::RECT as SysRect,
    Graphics::Gdi::{GetDC, ReleaseDC},
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{GetClientRect, SendMessageW},
    },
};

const WM_USER: u32 = 0x0400;
const EM_CHARFROMPOS: u32 = 0x00D7;
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_GETOLEINTERFACE: u32 = WM_USER + 60;
const EM_GETTEXTRANGE: u32 = WM_USER + 75;
const WM_GETTEXTLENGTH: u32 = 0x000E;
/// tom.h: tomStart | tomClientCoord with TA_BASELINE | TA_LEFT.
const POINT_FLAGS: tomConstants = tomConstants(32 | 256 | 24);

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct CharRange {
    pub min: i32,
    pub max: i32,
}
#[repr(C)]
struct TextRange {
    range: CharRange,
    text: *mut u16,
}

/// One complete emoji grapheme in UTF-16 units, with its first code point length.
#[derive(Debug, PartialEq, Eq)]
pub struct Cluster {
    pub units: Range<usize>,
    pub first: usize,
}

/// Finds the emoji in UTF-16 text.
pub fn clusters(text: &[u16]) -> Vec<Cluster> {
    let decoded = String::from_utf16_lossy(text);
    let mut position = 0;
    let mut found = Vec::new();
    for grapheme in decoded.graphemes(true) {
        let length = grapheme.encode_utf16().count();
        let mut chars = grapheme.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        let regional_pair = is_regional(first) && chars.next().is_some_and(is_regional);
        let keycap = matches!(first, '0'..='9' | '#' | '*') && grapheme.ends_with('\u{20E3}');
        if !grapheme.contains('\u{FE0E}')
            && (is_emoji(first)
                || regional_pair
                || keycap
                || (is_text_emoji(first) && grapheme.contains('\u{FE0F}')))
        {
            found.push(Cluster {
                units: position..position + length,
                first: first.len_utf16(),
            });
        }
        position += length;
    }
    found
}
fn is_regional(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}
/// Characters shown as emoji even without U+FE0F.
fn is_emoji(c: char) -> bool {
    match c as u32 {
        0x1F1E6..=0x1F1FF | 0x1F3FB..=0x1F3FF => false,
        0x1F004 | 0x1F0CF | 0x1F18E | 0x1F191..=0x1F19A | 0x1F201 | 0x1F21A | 0x1F22F => true,
        0x1F232..=0x1F236 | 0x1F238..=0x1F23A | 0x1F250 | 0x1F251 => true,
        0x1F300..=0x1FAFF => true,
        0x231A | 0x231B | 0x23E9..=0x23EC | 0x23F0 | 0x23F3 | 0x25FD | 0x25FE => true,
        0x2614 | 0x2615 | 0x2648..=0x2653 | 0x267F | 0x2693 | 0x26A1 | 0x26AA | 0x26AB => true,
        0x26BD | 0x26BE | 0x26C4 | 0x26C5 | 0x26CE | 0x26D4 | 0x26EA | 0x26F2 | 0x26F3 => true,
        0x26F5 | 0x26FA | 0x26FD | 0x2705 | 0x270A | 0x270B | 0x2728 | 0x274C | 0x274E => true,
        0x2753..=0x2755 | 0x2757 | 0x2795..=0x2797 | 0x27B0 | 0x27BF => true,
        0x2B1B | 0x2B1C | 0x2B50 | 0x2B55 => true,
        _ => false,
    }
}
/// Characters that are text by default and emoji when followed by U+FE0F.
fn is_text_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0xA9 | 0xAE
            | 0x203C
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x2194..=0x21AA
            | 0x2300..=0x23FF
            | 0x24C2
            | 0x25AA..=0x25FE
            | 0x2600..=0x27BF
            | 0x2934
            | 0x2935
            | 0x2B05..=0x2B55
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
            | 0x1F000..=0x1F2FF
    )
}

struct Painter {
    dwrite: IDWriteFactory,
    target: ID2D1DCRenderTarget,
}
#[derive(Default)]
struct PainterCache {
    painter: Option<Painter>,
    busy: bool,
}
thread_local! {
    static PAINTER: RefCell<PainterCache> = const { RefCell::new(PainterCache { painter: None, busy: false }) };
}
struct PainterLease {
    painter: Option<Painter>,
    healthy: bool,
}
impl PainterLease {
    fn take() -> Option<Self> {
        let painter = PAINTER
            .try_with(|cell| {
                let mut cache = cell.try_borrow_mut().ok()?;
                if cache.busy {
                    return None;
                }
                cache.busy = true;
                Some(cache.painter.take())
            })
            .ok()??;
        Some(Self {
            painter,
            healthy: true,
        })
    }
}
impl Drop for PainterLease {
    fn drop(&mut self) {
        let _ = PAINTER.try_with(|cell| {
            if let Ok(mut cache) = cell.try_borrow_mut() {
                cache.painter = if self.healthy {
                    self.painter.take()
                } else {
                    None
                };
                cache.busy = false;
            }
        });
    }
}
impl Painter {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: plain factory construction with valid, fully initialized arguments.
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                // 96 DPI makes one Direct2D unit one client pixel, matching RichEdit's points.
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = d2d.CreateDCRenderTarget(&properties)?;
            Ok(Self { dwrite, target })
        }
    }
    fn format(&self, size: f32) -> windows::core::Result<IDWriteTextFormat> {
        // SAFETY: constant, terminated font and locale names.
        unsafe {
            self.dwrite.CreateTextFormat(
                w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )
        }
    }
    fn measure(
        &self,
        text: &[u16],
        format: &IDWriteTextFormat,
    ) -> windows::core::Result<GlyphMetrics> {
        // SAFETY: the layout borrows only these live arguments during the calls.
        unsafe {
            let layout = self
                .dwrite
                .CreateTextLayout(text, format, 100_000., 100_000.)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            let mut lines = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0;
            layout.GetLineMetrics(Some(&mut lines), &mut count)?;
            let overhang = layout.GetOverhangMetrics()?;
            Ok(GlyphMetrics {
                width: metrics.widthIncludingTrailingWhitespace,
                height: metrics.height,
                baseline: lines[0].baseline,
                ink: D2D_RECT_F {
                    left: -overhang.left,
                    top: -overhang.top,
                    right: metrics.layoutWidth + overhang.right,
                    bottom: metrics.layoutHeight + overhang.bottom,
                },
            })
        }
    }
}

struct GlyphMetrics {
    width: f32,
    height: f32,
    baseline: f32,
    ink: D2D_RECT_F,
}
struct GlyphDraw {
    text: Vec<u16>,
    format: IDWriteTextFormat,
    layout: D2D_RECT_F,
    clip: D2D_RECT_F,
    background: ID2D1SolidColorBrush,
    ink: ID2D1SolidColorBrush,
}
struct WindowDc {
    hwnd: *mut c_void,
    dc: *mut c_void,
}
impl Drop for WindowDc {
    fn drop(&mut self) {
        // SAFETY: this guard owns the matching GetDC result on the same window.
        unsafe {
            ReleaseDC(self.hwnd, self.dc);
        }
    }
}
struct DrawSession<'a> {
    target: &'a ID2D1DCRenderTarget,
    active: bool,
}
impl<'a> DrawSession<'a> {
    fn begin(target: &'a ID2D1DCRenderTarget) -> Self {
        // SAFETY: BindDC and all fallible layout/brush preparation finished first.
        unsafe {
            target.BeginDraw();
        }
        Self {
            target,
            active: true,
        }
    }
    fn finish(mut self) -> windows::core::Result<()> {
        self.active = false;
        // SAFETY: this guard started exactly one draw session on the live target.
        unsafe { self.target.EndDraw(None, None) }
    }
}
impl Drop for DrawSession<'_> {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: an early return or unwind must close the active session.
            let _ = unsafe { self.target.EndDraw(None, None) };
        }
    }
}

fn send(hwnd: *mut c_void, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: synchronous messages to the live editor window with valid pointers.
    unsafe { SendMessageW(hwnd, message, wparam, lparam) }
}
pub fn selection(hwnd: *mut c_void) -> CharRange {
    let mut range = CharRange::default();
    send(
        hwnd,
        EM_EXGETSEL,
        0,
        (&mut range as *mut CharRange) as isize,
    );
    range
}
fn char_at(hwnd: *mut c_void, x: i32, y: i32) -> i32 {
    let point = [x, y];
    send(hwnd, EM_CHARFROMPOS, 0, point.as_ptr() as isize) as i32
}
fn color(rgb: u32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 255) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
}

/// Draws color emoji using the document's fixed font size, DPI and zoom. Selected
/// clusters and any region whose original background is uncertain stay native.
pub fn paint(hwnd: *mut c_void, paper: u32) -> bool {
    // A nested WM_PAINT must neither borrow the active painter nor create another.
    let Some(mut lease) = PainterLease::take() else {
        return false;
    };
    let mut client = SysRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a live window and initialized writable rectangle.
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 || client.right <= 0 || client.bottom <= 0 {
        return false;
    }
    let length = send(hwnd, WM_GETTEXTLENGTH, 0, 0).max(0) as i32;
    let first = (char_at(hwnd, 0, 0) - 16).max(0);
    let last = (char_at(hwnd, client.right, client.bottom) + 64).min(length);
    if last <= first {
        return false;
    }
    let mut units = vec![0_u16; (last - first) as usize + 1];
    let mut request = TextRange {
        range: CharRange {
            min: first,
            max: last,
        },
        text: units.as_mut_ptr(),
    };
    let copied = send(
        hwnd,
        EM_GETTEXTRANGE,
        0,
        (&mut request as *mut TextRange) as isize,
    )
    .max(0) as usize;
    units.truncate(copied.min(units.len()));
    let found = clusters(&units);
    if found.is_empty() {
        return false;
    }
    let selected = selection(hwnd);
    lease.healthy = false;
    let result = (|| -> windows::core::Result<()> {
        if lease.painter.is_none() {
            lease.painter = Some(Painter::new()?);
        }
        let Some(painter) = lease.painter.as_ref() else {
            return Ok(());
        };
        let mut ole: *mut c_void = ptr::null_mut();
        send(
            hwnd,
            EM_GETOLEINTERFACE,
            0,
            (&mut ole as *mut *mut c_void) as isize,
        );
        if ole.is_null() {
            return Ok(());
        }
        // SAFETY: EM_GETOLEINTERFACE returned an owned COM reference.
        let document: ITextDocument = unsafe { IUnknown::from_raw(ole) }.cast()?;
        // SAFETY: obtains DPI for this live per-monitor-aware window.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let (mut numerator, mut denominator) = (0_u32, 0_u32);
        let zoom = send(
            hwnd,
            WM_USER + 224,
            (&mut numerator as *mut u32) as usize,
            (&mut denominator as *mut u32) as isize,
        );
        if zoom == 0 || (numerator == 0 && denominator == 0) {
            numerator = 1;
            denominator = 1;
        }
        // SAFETY: the window DC is owned by the guard through every return path.
        let dc = unsafe { GetDC(hwnd) };
        if dc.is_null() {
            return Ok(());
        }
        let dc = WindowDc { hwnd, dc };
        let target = &painter.target;
        let bounds = RECT {
            left: client.left,
            top: client.top,
            right: client.right,
            bottom: client.bottom,
        };
        // SAFETY: this window DC and rectangle remain valid until the guard drops.
        unsafe {
            target.BindDC(HDC(dc.dc), &bounds)?;
        }
        let visible = D2D_RECT_F {
            left: client.left as f32,
            top: client.top as f32,
            right: client.right as f32,
            bottom: client.bottom as f32,
        };
        let mut draws = Vec::new();
        for cluster in &found {
            let start = first + cluster.units.start as i32;
            let end = first + cluster.units.end as i32;
            if selected.min < selected.max && selected.min < end && start < selected.max {
                continue;
            }
            // SAFETY: these non-mutating TOM ranges never alter the user's caret,
            // selection or document formatting, including inside joined graphemes.
            let range = match unsafe { document.Range(start, end) } {
                Ok(range) => range,
                Err(_) => continue,
            };
            let Some(background) = range_background(&range, paper) else {
                continue;
            };
            // SAFETY: the live font object describes this entire emoji range.
            let points = match unsafe { range.GetFont().and_then(|font| font.GetSize()) } {
                Ok(points) => points,
                Err(_) => continue,
            };
            let Some(pixel_size) = font_pixels(points, dpi, numerator, denominator) else {
                continue;
            };
            let (
                Some((x0, baseline)),
                Some((x_end, end_baseline)),
                Some((_, top)),
                Some((_, bottom)),
            ) = (
                range_point(&range, POINT_FLAGS),
                range_point(&range, tomConstants(256 | 24)),
                range_point(&range, tomConstants(32 | 256)),
                range_point(&range, tomConstants(32 | 256 | 8)),
            )
            else {
                continue;
            };
            // Wrapped or ambiguous advances remain native; sizing never consults
            // the caret position inside the first code point of a joined emoji.
            if end_baseline != baseline || x_end <= x0 || bottom <= top {
                continue;
            }
            let text = &units[cluster.units.clone()];
            let mut format = painter.format(pixel_size)?;
            let mut metrics = painter.measure(text, &format)?;
            let reserved_width = x_end - x0;
            if !metrics.width.is_finite() || metrics.width <= 0. {
                continue;
            }
            if metrics.width > reserved_width {
                format = painter.format(pixel_size * reserved_width / metrics.width)?;
                metrics = painter.measure(text, &format)?;
            }
            let origin_x = x0 + (reserved_width - metrics.width).max(0.) / 2.;
            let origin_y = baseline - metrics.baseline;
            let glyph = D2D_RECT_F {
                left: origin_x + metrics.ink.left,
                top: origin_y + metrics.ink.top,
                right: origin_x + metrics.ink.right,
                bottom: origin_y + metrics.ink.bottom,
            };
            let cell = D2D_RECT_F {
                left: x0,
                top,
                right: x_end,
                bottom,
            };
            let Some(clip) =
                intersect_rect(glyph, cell).and_then(|clip| intersect_rect(clip, visible))
            else {
                continue;
            };
            // All fallible font, layout and brush work completes before BeginDraw.
            // SAFETY: the render target is bound; these colors are fully initialized.
            let (background_brush, ink) = unsafe {
                let brush = target.CreateSolidColorBrush(&color(background), None)?;
                let dark =
                    ((background >> 16) & 255) + ((background >> 8) & 255) + (background & 255)
                        < 384;
                let ink = target
                    .CreateSolidColorBrush(&color(if dark { 0xe6e6e5 } else { 0x1a1a1a }), None)?;
                (brush, ink)
            };
            draws.push(GlyphDraw {
                text: text.to_vec(),
                format,
                layout: D2D_RECT_F {
                    left: origin_x,
                    top: origin_y,
                    right: origin_x + metrics.width + 1.,
                    bottom: origin_y + metrics.height,
                },
                clip,
                background: background_brush,
                ink,
            });
        }
        if draws.is_empty() {
            return Ok(());
        }
        let session = DrawSession::begin(target);
        for draw in draws {
            // SAFETY: every brush/layout is prepared above. The clip intersects
            // actual glyph ink, the reserved native cell and the visible client.
            unsafe {
                target.PushAxisAlignedClip(&draw.clip, D2D1_ANTIALIAS_MODE_ALIASED);
                target.FillRectangle(&draw.clip, &draw.background);
                target.DrawText(
                    &draw.text,
                    &draw.format,
                    &draw.layout,
                    &draw.ink,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT | D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                target.PopAxisAlignedClip();
            }
        }
        session.finish()
    })();
    lease.healthy = result.is_ok();
    result.is_ok()
}

fn range_point(range: &ITextRange, flags: tomConstants) -> Option<(f32, f32)> {
    let (mut x, mut y) = (0, 0);
    // SAFETY: a live, non-mutating range and initialized output integers.
    unsafe {
        range.GetPoint(flags, &mut x, &mut y).ok()?;
    }
    Some((x as f32, y as f32))
}
fn range_background(range: &ITextRange, paper: u32) -> Option<u32> {
    // SAFETY: all interfaces are obtained from the live range; these calls only
    // inspect attributes. Unsupported or mixed properties conservatively skip it.
    unsafe {
        let background = range.GetFont().ok()?.GetBackColor().ok()?;
        let paragraph: ITextPara2 = range.GetPara().ok()?.cast().ok()?;
        let (mut effects, mut mask) = (0, 0);
        paragraph.GetEffects(&mut effects, &mut mask).ok()?;
        repaint_background(background, effects, mask, paper)
    }
}
fn repaint_background(background: i32, effects: i32, mask: i32, paper: u32) -> Option<u32> {
    let table = tomParaEffectTable.0 | tomParaEffectTableRowDelimiter.0;
    if mask & table != table || effects & table != 0 {
        return None;
    }
    if background == tomAutoColor.0 {
        return (paper <= 0xffffff).then_some(paper);
    }
    // COLORREF's high byte must be zero. Mixed values, palette indices and
    // unknown automatic/system values cannot safely erase a native rectangle.
    if !(0..=0xffffff).contains(&background) {
        return None;
    }
    let native = background as u32;
    Some(((native & 255) << 16) | (native & 0xff00) | ((native >> 16) & 255))
}
fn font_pixels(points: f32, dpi: u32, numerator: u32, denominator: u32) -> Option<f32> {
    if !points.is_finite() || points <= 0. || dpi == 0 || numerator == 0 || denominator == 0 {
        return None;
    }
    let pixels = points * dpi as f32 / 72. * numerator as f32 / denominator as f32;
    (pixels.is_finite() && pixels > 0.).then_some(pixels)
}
fn intersect_rect(a: D2D_RECT_F, b: D2D_RECT_F) -> Option<D2D_RECT_F> {
    if [
        a.left, a.top, a.right, a.bottom, b.left, b.top, b.right, b.bottom,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return None;
    }
    let result = D2D_RECT_F {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (result.left < result.right && result.top < result.bottom).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(usize, usize, usize)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        clusters(&units)
            .into_iter()
            .map(|cluster| (cluster.units.start, cluster.units.end, cluster.first))
            .collect()
    }

    #[test]
    fn text_presentation_selectors_keep_native_glyphs() {
        assert!(found("😀\u{fe0e}").is_empty());
        assert!(found("👩\u{fe0e}\u{200d}💻").is_empty());
        assert_eq!(found("⭐\u{fe0e} ✅"), [(3, 4, 1)]);
        assert_eq!(
            found("1\u{20e3}"),
            [(0, 2, 1)],
            "keycap without redundant VS16"
        );
    }

    #[test]
    fn emoji_metrics_are_document_size_dpi_and_zoom_not_internal_caret_widths() {
        assert_eq!(font_pixels(12., 96, 1, 1), Some(16.));
        assert_eq!(font_pixels(12., 144, 150, 100), Some(36.));
        assert_eq!(font_pixels(18., 192, 1, 2), Some(24.));
        for invalid in [0., -9_999_999., f32::NAN, f32::INFINITY] {
            assert_eq!(font_pixels(invalid, 96, 1, 1), None);
        }
        assert_eq!(font_pixels(12., 0, 1, 1), None);
        assert_eq!(font_pixels(12., 96, 1, 0), None);
    }

    #[test]
    fn emoji_repainting_uses_the_actual_uniform_background_or_skips() {
        let stable = tomParaEffectTable.0 | tomParaEffectTableRowDelimiter.0;
        assert_eq!(
            repaint_background(0x00332211, 0, stable, 0xffffff),
            Some(0x112233)
        );
        assert_eq!(
            repaint_background(tomAutoColor.0, 0, stable, 0x202020),
            Some(0x202020)
        );
        assert_eq!(
            repaint_background(-9_999_999, 0, stable, 0xffffff),
            None,
            "mixed background"
        );
        assert_eq!(
            repaint_background(0x01000012, 0, stable, 0xffffff),
            None,
            "palette index"
        );
        assert_eq!(
            repaint_background(0x00332211, tomParaEffectTable.0, stable, 0xffffff),
            None,
            "table cell"
        );
        assert_eq!(
            repaint_background(0x00332211, 0, 0, 0xffffff),
            None,
            "unknown paragraph effects"
        );
        assert_eq!(
            repaint_background(0x00332211, 0, tomParaEffectTable.0, 0xffffff),
            None,
            "mixed table boundary"
        );
    }

    #[test]
    fn emoji_clip_cannot_cover_a_neighbor_or_leave_the_client() {
        let ink = D2D_RECT_F {
            left: 8.,
            top: -4.,
            right: 40.,
            bottom: 30.,
        };
        let cell = D2D_RECT_F {
            left: 10.,
            top: -2.,
            right: 25.,
            bottom: 22.,
        };
        let client = D2D_RECT_F {
            left: 0.,
            top: 0.,
            right: 100.,
            bottom: 20.,
        };
        let clipped = intersect_rect(ink, cell)
            .and_then(|rect| intersect_rect(rect, client))
            .expect("visible glyph");
        assert_eq!(
            (clipped.left, clipped.top, clipped.right, clipped.bottom),
            (10., 0., 25., 20.)
        );
        let outside = D2D_RECT_F {
            left: 101.,
            top: 0.,
            right: 110.,
            bottom: 20.,
        };
        assert!(intersect_rect(outside, client).is_none());
        let unknown = D2D_RECT_F {
            left: f32::NAN,
            ..ink
        };
        assert!(intersect_rect(unknown, client).is_none());
    }

    #[test]
    fn reentrant_paints_do_not_borrow_or_replace_an_active_painter() {
        let lease = PainterLease::take().expect("idle painter cache");
        assert!(
            PainterLease::take().is_none(),
            "reentry leaves native drawing intact"
        );
        drop(lease);
        assert!(
            PainterLease::take().is_some(),
            "completion releases the busy flag"
        );
    }
    #[test]
    fn finds_whole_emoji_sequences() {
        assert_eq!(found("a😀b"), [(1, 3, 2)]);
        assert_eq!(found("👍🏽"), [(0, 4, 2)], "skin tone joins its base");
        assert_eq!(
            found("🇳🇬"),
            [(0, 4, 2)],
            "a flag is two regional indicators"
        );
        assert_eq!(found("👨‍👩‍👧"), [(0, 8, 2)], "a family is one picture");
        assert_eq!(found("1️⃣"), [(0, 3, 1)], "keycap");
        assert_eq!(
            found("❤️ ok"),
            [(0, 2, 1)],
            "text heart with emoji selector"
        );
        assert!(
            found("❤ © plain").is_empty(),
            "text presentation stays text"
        );
        assert_eq!(found("⭐x✅"), [(0, 1, 1), (2, 3, 1)]);
    }
}
