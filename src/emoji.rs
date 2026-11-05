//! Offline color emoji over RichEdit's native text. Whole graphemes are shaped with
//! the bundled Noto font, decoded from its CBDT PNG strikes and drawn as premultiplied
//! bitmaps. The underlying Unicode, document formatting and selection stay untouched.
use rustybuzz::ttf_parser::{GlyphId, RasterImageFormat};
use std::{cell::RefCell, collections::VecDeque, ffi::c_void, ops::Range, ptr, rc::Rc};
use unicode_segmentation::UnicodeSegmentation as _;
use windows::{
    Win32::{
        Foundation::{E_FAIL, RECT},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED,
                    D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                D2D1_BITMAP_PROPERTIES, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
                ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
        UI::Controls::RichEdit::{
            ITextDocument, ITextPara2, ITextRange, tomAutoColor, tomConstants, tomParaEffectTable,
            tomParaEffectTableRowDelimiter,
        },
    },
    core::{IUnknown, Interface},
};
use windows_sys::Win32::{
    Foundation::RECT as SysRect,
    Graphics::Gdi::{GetDC, GetPixel, ReleaseDC},
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{GetClientRect, HideCaret, SendMessageW, ShowCaret},
    },
};

const NOTO_FONT: &[u8] = include_bytes!("../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
const GLYPH_CACHE_CAPACITY: usize = 128;

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

/// All geometry is in ems, with a downward-positive y axis and the baseline at zero.
#[derive(Clone, Copy)]
struct RunMetrics {
    advance: f32,
    ink: D2D_RECT_F,
}
struct RasterGlyph {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    bounds: D2D_RECT_F,
}
struct RasterRun {
    glyphs: Vec<RasterGlyph>,
    metrics: RunMetrics,
}
struct BitmapGlyph {
    bitmap: ID2D1Bitmap,
    bounds: D2D_RECT_F,
}
struct GlyphRun {
    glyphs: Vec<BitmapGlyph>,
    metrics: RunMetrics,
}

/// Shapes the entire grapheme, including skin tones, flags and ZWJ ligatures.
/// An unsupported or undecodable glyph leaves the whole native grapheme intact.
fn rasterize(face: &rustybuzz::Face<'_>, text: &str) -> Option<RasterRun> {
    if text.is_empty() || text.len() > 256 || text.contains('\u{fe0e}') {
        return None;
    }
    let em = face.units_per_em() as f32;
    if em <= 0. {
        return None;
    }
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let shaped = rustybuzz::shape(face, &[], buffer);
    if shaped.glyph_infos().is_empty() || shaped.glyph_infos().len() > 16 {
        return None;
    }
    let (mut pen_x, mut pen_y) = (0_f32, 0_f32);
    let mut glyphs = Vec::new();
    let mut ink: Option<D2D_RECT_F> = None;
    for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
        let id = u16::try_from(info.glyph_id).ok()?;
        if id == 0 {
            return None;
        }
        let raster = face.glyph_raster_image(GlyphId(id), u16::MAX);
        let Some(raster) = raster else {
            // HarfBuzz hides default-ignorables as zero-advance space glyphs.
            if position.x_advance == 0 && position.y_advance == 0 {
                continue;
            }
            return None;
        };
        if raster.format != RasterImageFormat::PNG
            || raster.pixels_per_em == 0
            || raster.width == 0
            || raster.height == 0
            || raster.width > 512
            || raster.height > 512
        {
            return None;
        }
        let image = image::load_from_memory_with_format(raster.data, image::ImageFormat::Png)
            .ok()?
            .into_rgba8();
        let (width, height) = image.dimensions();
        if width != u32::from(raster.width) || height != u32::from(raster.height) {
            return None;
        }
        let mut pixels = image.into_raw();
        premultiply_rgba(&mut pixels);
        let strike = f32::from(raster.pixels_per_em);
        let left = (pen_x + position.x_offset as f32) / em + f32::from(raster.x) / strike;
        // ttf-parser normalizes CBDT y to the bitmap's bottom bearing (y-up).
        let bottom = -(pen_y + position.y_offset as f32) / em - f32::from(raster.y) / strike;
        let bounds = D2D_RECT_F {
            left,
            top: bottom - height as f32 / strike,
            right: left + width as f32 / strike,
            bottom,
        };
        ink = Some(match ink {
            Some(ink) => D2D_RECT_F {
                left: ink.left.min(bounds.left),
                top: ink.top.min(bounds.top),
                right: ink.right.max(bounds.right),
                bottom: ink.bottom.max(bounds.bottom),
            },
            None => bounds,
        });
        glyphs.push(RasterGlyph {
            pixels,
            width,
            height,
            bounds,
        });
        pen_x += position.x_advance as f32;
        pen_y += position.y_advance as f32;
    }
    let advance = pen_x / em;
    if !advance.is_finite() || advance <= 0. || pen_y != 0. {
        return None;
    }
    Some(RasterRun {
        glyphs,
        metrics: RunMetrics { advance, ink: ink? },
    })
}

/// PNG has straight RGBA; Direct2D's BGRA bitmaps require premultiplied channels.
fn premultiply_rgba(pixels: &mut [u8]) {
    let channel = |value: u8, alpha: u8| ((u16::from(value) * u16::from(alpha) + 127) / 255) as u8;
    for pixel in pixels.chunks_exact_mut(4) {
        let [r, g, b, a] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        pixel.copy_from_slice(&[channel(b, a), channel(g, a), channel(r, a), a]);
    }
}

struct Painter {
    target: ID2D1DCRenderTarget,
    face: rustybuzz::Face<'static>,
    // Both successful and unsupported sequences are cached. Each entry is one
    // whole grapheme; retaining at most 128 prevents an unbounded GPU cache.
    cache: VecDeque<(String, Option<Rc<GlyphRun>>)>,
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
        let face = rustybuzz::Face::from_slice(NOTO_FONT, 0)
            .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
        // SAFETY: plain factory construction with valid, initialized arguments.
        let target = unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                // One Direct2D unit is one client pixel; DPI is applied to font size.
                dpiX: 96.,
                dpiY: 96.,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            })?
        };
        Ok(Self {
            target,
            face,
            cache: VecDeque::new(),
        })
    }

    fn glyph_run(&mut self, text: &str) -> windows::core::Result<Option<Rc<GlyphRun>>> {
        if let Some(index) = self.cache.iter().position(|(key, _)| key == text)
            && let Some(entry) = self.cache.remove(index)
        {
            let run = entry.1.clone();
            self.cache.push_back(entry);
            return Ok(run);
        }
        let run = if let Some(raster) = rasterize(&self.face, text) {
            let properties = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.,
                dpiY: 96.,
            };
            let mut glyphs = Vec::with_capacity(raster.glyphs.len());
            for glyph in raster.glyphs {
                // SAFETY: the buffer contains width*height BGRA pixels. CreateBitmap
                // copies it before returning; the resulting COM object owns its data.
                let bitmap = unsafe {
                    self.target.CreateBitmap(
                        D2D_SIZE_U {
                            width: glyph.width,
                            height: glyph.height,
                        },
                        Some(glyph.pixels.as_ptr().cast()),
                        glyph.width * 4,
                        &properties,
                    )?
                };
                glyphs.push(BitmapGlyph {
                    bitmap,
                    bounds: glyph.bounds,
                });
            }
            Some(Rc::new(GlyphRun {
                glyphs,
                metrics: raster.metrics,
            }))
        } else {
            None
        };
        if self.cache.len() == GLYPH_CACHE_CAPACITY {
            self.cache.pop_front();
        }
        self.cache.push_back((text.to_owned(), run.clone()));
        Ok(run)
    }
}

struct GlyphPlacement {
    origin_x: f32,
    baseline: f32,
    scale: f32,
}
impl GlyphPlacement {
    fn bounds(&self, bounds: D2D_RECT_F) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.origin_x + bounds.left * self.scale,
            top: self.baseline + bounds.top * self.scale,
            right: self.origin_x + bounds.right * self.scale,
            bottom: self.baseline + bounds.bottom * self.scale,
        }
    }
}

fn glyph_placement(
    metrics: RunMetrics,
    cell: D2D_RECT_F,
    baseline: f32,
    font_pixels: f32,
) -> Option<GlyphPlacement> {
    let left = metrics.ink.left.min(0.);
    let right = metrics.ink.right.max(metrics.advance);
    let width = right - left;
    let height = metrics.ink.bottom - metrics.ink.top;
    if [
        left,
        right,
        width,
        height,
        baseline,
        font_pixels,
        cell.left,
        cell.top,
        cell.right,
        cell.bottom,
    ]
    .iter()
    .any(|value| !value.is_finite())
        || width <= 0.
        || height <= 0.
        || font_pixels <= 0.
        || cell.left >= cell.right
        || cell.top >= cell.bottom
    {
        return None;
    }
    // Font size, DPI and zoom determine the desired size. Only the complete
    // native cluster's cell constrains it, never an internal code-point caret.
    let scale = font_pixels
        .min((cell.right - cell.left) / width)
        .min((cell.bottom - cell.top) / height);
    let low = cell.top - metrics.ink.top * scale;
    let high = (cell.bottom - metrics.ink.bottom * scale).max(low);
    Some(GlyphPlacement {
        origin_x: cell.left + ((cell.right - cell.left) - width * scale) / 2. - left * scale,
        baseline: baseline.clamp(low, high),
        scale,
    })
}

struct GlyphDraw {
    run: Rc<GlyphRun>,
    placement: GlyphPlacement,
    clip: D2D_RECT_F,
    background: ID2D1SolidColorBrush,
}
struct WindowDc {
    hwnd: *mut c_void,
    dc: *mut c_void,
}
impl Drop for WindowDc {
    fn drop(&mut self) {
        // SAFETY: owns the matching GetDC result on the same live window.
        unsafe {
            ReleaseDC(self.hwnd, self.dc);
        }
    }
}
struct HiddenCaret {
    hwnd: *mut c_void,
    hidden: bool,
}
impl HiddenCaret {
    fn new(hwnd: *mut c_void) -> Self {
        // SAFETY: increments the hide count only for this live window's caret.
        let hidden = unsafe { HideCaret(hwnd) } != 0;
        Self { hwnd, hidden }
    }
}
impl Drop for HiddenCaret {
    fn drop(&mut self) {
        if self.hidden {
            // SAFETY: balances our successful HideCaret, preserving its prior state.
            unsafe {
                ShowCaret(self.hwnd);
            }
        }
    }
}
struct DrawSession<'a> {
    target: &'a ID2D1DCRenderTarget,
    active: bool,
}
impl<'a> DrawSession<'a> {
    fn begin(target: &'a ID2D1DCRenderTarget) -> Self {
        // SAFETY: BindDC and every fallible bitmap/brush preparation finished first.
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
        // SAFETY: this guard started exactly one session on this live target.
        unsafe { self.target.EndDraw(None, None) }
    }
}
impl Drop for DrawSession<'_> {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: early return/unwind must also pair BeginDraw with EndDraw.
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
    let channel = |shift: u32| ((rgb >> shift) & 255) as f32 / 255.;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.,
    }
}
fn colorref_rgb(native: u32) -> u32 {
    ((native & 255) << 16) | (native & 0xff00) | ((native >> 16) & 255)
}

#[derive(Debug, PartialEq, Eq)]
enum ClusterSelection {
    Unselected,
    Selected,
    Partial,
}
fn cluster_selection(cluster: CharRange, selected: CharRange) -> ClusterSelection {
    if selected.min >= selected.max || selected.max <= cluster.min || selected.min >= cluster.max {
        ClusterSelection::Unselected
    } else if selected.min <= cluster.min && selected.max >= cluster.max {
        ClusterSelection::Selected
    } else {
        ClusterSelection::Partial
    }
}

/// A three-quarter majority of at least eight edge samples is required. Invalid
/// DC pixels count against confidence, and a tie or mixed highlight stays native.
fn sampled_background(samples: &[u32]) -> Option<u32> {
    let candidate = samples
        .iter()
        .copied()
        .filter(|sample| *sample <= 0xffffff)
        .max_by_key(|sample| samples.iter().filter(|other| *other == sample).count())?;
    let count = samples
        .iter()
        .filter(|sample| **sample == candidate)
        .count();
    (count >= 8 && count * 4 >= samples.len() * 3).then(|| colorref_rgb(candidate))
}

fn selection_background(dc: *mut c_void, cell: D2D_RECT_F) -> Option<u32> {
    let left = cell.left.ceil() as i32;
    let right = cell.right.floor() as i32 - 1;
    let top = cell.top.ceil() as i32;
    let bottom = cell.bottom.floor() as i32 - 1;
    if right < left || bottom < top {
        return None;
    }
    let mut points = Vec::with_capacity(20);
    for part in 0..=5 {
        let x = left + (right - left) * part / 5;
        points.push((x, top));
        points.push((x, bottom));
    }
    for part in 1..=3 {
        let y = top + (bottom - top) * part / 4;
        points.push((left, y));
        points.push((right, y));
    }
    points.sort_unstable();
    points.dedup();
    let samples: Vec<_> = points
        .into_iter()
        .map(|(x, y)| {
            // SAFETY: every point is inside the clipped live client cell. GetPixel
            // failure returns CLR_INVALID, rejected by sampled_background.
            unsafe { GetPixel(dc, x, y) }
        })
        .collect();
    sampled_background(&samples)
}

/// Overlays bundled color bitmaps without changing native text or selection.
/// Partial selections, mixed backgrounds and table/ambiguous cells stay native.
pub fn paint(hwnd: *mut c_void, paper: u32) -> bool {
    // No RefCell borrow survives a native call, and a nested paint stays native.
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
    let length = send(hwnd, WM_GETTEXTLENGTH, 0, 0).clamp(0, i32::MAX as isize) as i32;
    let first = char_at(hwnd, 0, 0).saturating_sub(64).max(0);
    let last = char_at(hwnd, client.right, client.bottom)
        .saturating_add(128)
        .min(length);
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
    units.truncate(copied.min(units.len().saturating_sub(1)));
    let found = clusters(&units);
    if found.is_empty() {
        return false;
    }
    let selected = selection(hwnd);
    let mut drawing = false;
    lease.healthy = false;
    let result = (|| -> windows::core::Result<bool> {
        if lease.painter.is_none() {
            lease.painter = Some(Painter::new()?);
        }
        let Some(painter) = lease.painter.as_mut() else {
            return Ok(false);
        };
        let mut ole: *mut c_void = ptr::null_mut();
        send(
            hwnd,
            EM_GETOLEINTERFACE,
            0,
            (&mut ole as *mut *mut c_void) as isize,
        );
        if ole.is_null() {
            return Ok(false);
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
        // SAFETY: the guard owns this window DC through every return path.
        let dc = unsafe { GetDC(hwnd) };
        if dc.is_null() {
            return Ok(false);
        }
        let dc = WindowDc { hwnd, dc };
        // COM clone allows cache mutation without holding a target-field borrow.
        let target = painter.target.clone();
        let bounds = RECT {
            left: client.left,
            top: client.top,
            right: client.right,
            bottom: client.bottom,
        };
        // SAFETY: the DC and rectangle remain live until the guard drops.
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
        for cluster in found {
            // A sliced boundary grapheme may be incomplete; never repaint it.
            if (first > 0 && cluster.units.start == 0)
                || (last < length && cluster.units.end == units.len())
            {
                continue;
            }
            let start = first + cluster.units.start as i32;
            let end = first + cluster.units.end as i32;
            let selected = cluster_selection(
                CharRange {
                    min: start,
                    max: end,
                },
                selected,
            );
            if selected == ClusterSelection::Partial {
                continue;
            }
            // SAFETY: non-mutating TOM ranges never move the user's selection.
            let range = match unsafe { document.Range(start, end) } {
                Ok(range) => range,
                Err(_) => continue,
            };
            let Some(mut background) = range_background(&range, paper) else {
                continue;
            };
            // SAFETY: reads the actual point size of the complete native range.
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
            if end_baseline != baseline || x_end <= x0 || bottom <= top {
                continue;
            }
            let cell = D2D_RECT_F {
                left: x0,
                top,
                right: x_end,
                bottom,
            };
            let Some(clip) = intersect_rect(cell, visible) else {
                continue;
            };
            if selected == ClusterSelection::Selected {
                // RichEdit can use inactive/acetate/custom selection colors. Read
                // its actual paint instead of assuming COLOR_HIGHLIGHT or paper.
                let Some(sampled) = selection_background(dc.dc, clip) else {
                    continue;
                };
                background = sampled;
            }
            let Ok(text) = String::from_utf16(&units[cluster.units]) else {
                continue;
            };
            let Some(run) = painter.glyph_run(&text)? else {
                continue;
            };
            let Some(placement) = glyph_placement(run.metrics, cell, baseline, pixel_size) else {
                continue;
            };
            // SAFETY: fully initialized opaque brush on the bound live target.
            let brush = unsafe { target.CreateSolidColorBrush(&color(background), None)? };
            draws.push(GlyphDraw {
                run,
                placement,
                clip,
                background: brush,
            });
        }
        if draws.is_empty() {
            return Ok(false);
        }
        // Every fallible decode/bitmap/brush operation precedes BeginDraw.
        let _caret = HiddenCaret::new(hwnd);
        for draw in draws {
            // A DC render target may present its entire bound surface. Bind only
            // this cell, so even presentation cannot overwrite neighboring text.
            let bounds = RECT {
                left: draw.clip.left as i32,
                top: draw.clip.top as i32,
                right: draw.clip.right as i32,
                bottom: draw.clip.bottom as i32,
            };
            let clip = D2D_RECT_F {
                left: 0.,
                top: 0.,
                right: (bounds.right - bounds.left) as f32,
                bottom: (bounds.bottom - bounds.top) as f32,
            };
            // SAFETY: the clipped cell is inside the live DC, owned by dc above.
            unsafe {
                target.BindDC(HDC(dc.dc), &bounds)?;
            }
            drawing = true;
            let session = DrawSession::begin(&target);
            // SAFETY: bitmap/brush work is complete. Both the D2D clip and the
            // bound DC are exactly this cluster's reserved native cell.
            unsafe {
                target.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
                // Clear every native mono pixel before drawing transparent Noto ink.
                target.FillRectangle(&clip, &draw.background);
                for glyph in &draw.run.glyphs {
                    let destination = draw.placement.bounds(glyph.bounds);
                    let destination = D2D_RECT_F {
                        left: destination.left - draw.clip.left,
                        top: destination.top - draw.clip.top,
                        right: destination.right - draw.clip.left,
                        bottom: destination.bottom - draw.clip.top,
                    };
                    target.DrawBitmap(
                        &glyph.bitmap,
                        Some(&destination),
                        1.,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
                target.PopAxisAlignedClip();
            }
            session.finish()?;
        }
        Ok(true)
    })();
    lease.healthy = result.is_ok();
    if result.is_err() && drawing {
        // Restore native pixels synchronously while this lease remains busy, so
        // the nested WM_PAINT cannot retry the failing overlay or create a loop.
        // SAFETY: this live window is repainted only after EndDraw has completed.
        unsafe {
            windows_sys::Win32::Graphics::Gdi::RedrawWindow(
                hwnd,
                ptr::null(),
                ptr::null_mut(),
                windows_sys::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows_sys::Win32::Graphics::Gdi::RDW_UPDATENOW,
            );
        }
    }
    result.unwrap_or(false)
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

    fn noto_face() -> rustybuzz::Face<'static> {
        rustybuzz::Face::from_slice(NOTO_FONT, 0).expect("bundled and hash-verified Noto font")
    }

    #[test]
    fn noto_shapes_complete_supported_sequences_into_one_color_picture() {
        let face = noto_face();
        for text in ["😀", "👍🏽", "👨‍👩‍👧‍👦", "🇳🇬", "1️⃣", "1\u{20e3}", "❤️", "👩🏽‍💻", "🏳️‍🌈"]
        {
            let run = rasterize(&face, text).unwrap_or_else(|| panic!("Noto must render {text}"));
            assert_eq!(run.glyphs.len(), 1, "whole-grapheme ligature for {text}");
            assert!(run.metrics.advance > 0.);
            assert!(run.glyphs[0].width > 0 && run.glyphs[0].height > 0);
        }
    }

    #[test]
    fn noto_png_strike_contains_actual_multicolor_premultiplied_pixels() {
        let run = rasterize(&noto_face(), "😀").expect("smiling face color strike");
        let glyph = &run.glyphs[0];
        assert_eq!(
            glyph.pixels.len(),
            (glyph.width * glyph.height * 4) as usize
        );
        let mut colors = std::collections::HashSet::new();
        for pixel in glyph.pixels.chunks_exact(4) {
            assert!(pixel[..3].iter().all(|channel| *channel <= pixel[3]));
            if pixel[3] == 255 {
                colors.insert([pixel[0], pixel[1], pixel[2]]);
            }
        }
        assert!(colors.len() > 16, "a colored image, not a monochrome mask");
        assert!(colors.iter().any(|[b, g, r]| b != g && r != b));
    }

    #[test]
    fn noto_respects_explicit_text_presentation_and_unsupported_characters() {
        let face = noto_face();
        assert!(rasterize(&face, "😀\u{fe0e}").is_none());
        assert!(rasterize(&face, "❤\u{fe0e}").is_none());
        assert!(rasterize(&face, "\u{10ffff}").is_none());
        assert!(rasterize(&face, "").is_none());
        assert!(rasterize(&face, "❤️").is_some());
        assert!(found("❤ © plain").is_empty());
    }

    #[test]
    fn straight_rgba_is_swizzled_and_premultiplied_with_rounding() {
        let mut pixels = [255, 128, 64, 128, 10, 20, 30, 0, 1, 2, 3, 255];
        premultiply_rgba(&mut pixels);
        assert_eq!(pixels, [32, 64, 128, 128, 0, 0, 0, 0, 3, 2, 1, 255]);
    }

    #[test]
    fn full_selected_graphemes_render_but_partial_selection_stays_native() {
        let family = CharRange { min: 3, max: 14 };
        assert_eq!(
            cluster_selection(family, CharRange { min: 0, max: 100 }),
            ClusterSelection::Selected
        );
        assert_eq!(
            cluster_selection(family, family),
            ClusterSelection::Selected
        );
        assert_eq!(
            cluster_selection(family, CharRange { min: 4, max: 14 }),
            ClusterSelection::Partial
        );
        assert_eq!(
            cluster_selection(family, CharRange { min: 3, max: 13 }),
            ClusterSelection::Partial
        );
        assert_eq!(
            cluster_selection(family, CharRange { min: 4, max: 4 }),
            ClusterSelection::Unselected
        );
        assert_eq!(
            cluster_selection(family, CharRange { min: 14, max: 20 }),
            ClusterSelection::Unselected
        );
    }

    #[test]
    fn selection_background_requires_a_clear_native_pixel_majority() {
        let highlight = 0x00d77800; // COLORREF -> RGB 0078d7, not a hardcoded selection choice.
        let mut samples = vec![highlight; 14];
        samples.extend([0x00ffffff; 4]);
        assert_eq!(sampled_background(&samples), Some(0x0078d7));
        samples[0] = 0x00ffffff;
        assert_eq!(
            sampled_background(&samples),
            None,
            "mixed edges cannot be erased"
        );
        assert_eq!(
            sampled_background(&[highlight; 7]),
            None,
            "too few real pixels"
        );
        assert_eq!(sampled_background(&[highlight; 8]), Some(0x0078d7));
        assert_eq!(
            sampled_background(&[u32::MAX; 18]),
            None,
            "GetPixel failure"
        );
        let mut failed = vec![highlight; 8];
        failed.extend([u32::MAX; 8]);
        assert_eq!(
            sampled_background(&failed),
            None,
            "missing pixels count against confidence"
        );
        assert_eq!(
            sampled_background(&[0x00000000; 18]),
            Some(0),
            "black is valid"
        );
    }

    #[test]
    fn noto_geometry_uses_font_size_and_complete_reserved_cell_without_bouncing() {
        let metrics = rasterize(&noto_face(), "👩🏽‍💻")
            .expect("whole profession")
            .metrics;
        let cell = D2D_RECT_F {
            left: 100.,
            top: 20.,
            right: 120.,
            bottom: 44.,
        };
        let pixel_size = font_pixels(12., 144, 1, 1).expect("document point size");
        let first = glyph_placement(metrics, cell, 39., pixel_size).expect("visible geometry");
        let second = glyph_placement(metrics, cell, 39., pixel_size)
            .expect("same size after caret/selection change");
        assert_eq!(
            (first.origin_x, first.baseline, first.scale),
            (second.origin_x, second.baseline, second.scale)
        );
        let drawn = first.bounds(metrics.ink);
        let epsilon = 0.0001;
        assert!(drawn.left >= cell.left - epsilon && drawn.right <= cell.right + epsilon);
        assert!(drawn.top >= cell.top - epsilon && drawn.bottom <= cell.bottom + epsilon);
        assert!(first.scale <= pixel_size);
        let zoomed =
            glyph_placement(metrics, cell, 39., pixel_size * 10.).expect("fit large zoom to cell");
        let drawn = zoomed.bounds(metrics.ink);
        assert!(drawn.left >= cell.left - epsilon && drawn.right <= cell.right + epsilon);
        assert!(drawn.top >= cell.top - epsilon && drawn.bottom <= cell.bottom + epsilon);
        assert!(glyph_placement(metrics, cell, 39., f32::NAN).is_none());
    }
}
