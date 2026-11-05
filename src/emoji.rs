//! Color emoji. Windows' windowed RichEdit draws text through GDI, which has no color
//! glyphs, so after each paint the visible emoji are drawn again with DirectWrite's color
//! fonts, the way phones and browsers show them. Skin tones, flags, keycaps and joined
//! sequences (families, professions) become one picture.
use std::{cell::RefCell, ffi::c_void, ops::Range, ptr};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{
                Common::{D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
                ID2D1DCRenderTarget, ID2D1Factory,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS, DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
        UI::Controls::RichEdit::ITextDocument,
    },
    core::{IUnknown, Interface, w},
};
use windows_sys::Win32::{
    Foundation::RECT as SysRect,
    Graphics::Gdi::{GetDC, ReleaseDC},
    UI::WindowsAndMessaging::{GetClientRect, SendMessageW},
};

const WM_USER: u32 = 0x0400;
const EM_CHARFROMPOS: u32 = 0x00D7;
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_GETOLEINTERFACE: u32 = WM_USER + 60;
const EM_GETTEXTRANGE: u32 = WM_USER + 75;
const WM_GETTEXTLENGTH: u32 = 0x000E;
/// tom.h: tomStart | tomClientCoord with TA_BASELINE | TA_LEFT.
const POINT_FLAGS: i32 = 32 | 256 | 24;

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

/// One emoji picture in UTF-16 units: where it starts and ends, and the length of its
/// first code point, whose single-glyph width gives the size RichEdit drew it at.
#[derive(Debug, PartialEq, Eq)]
pub struct Cluster {
    pub units: Range<usize>,
    pub first: usize,
}

/// Finds the emoji in UTF-16 text.
pub fn clusters(text: &[u16]) -> Vec<Cluster> {
    let mut chars = Vec::with_capacity(text.len());
    let mut offset = 0;
    for decoded in char::decode_utf16(text.iter().copied()) {
        let c = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
        chars.push((offset, c));
        offset += c.len_utf16();
    }
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let position = |i: usize| chars.get(i).map_or(text.len(), |&(offset, _)| offset);
    let mut found = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        let end = if is_regional(c) && at(i + 1).is_some_and(is_regional) {
            i + 2
        } else if matches!(c, '0'..='9' | '#' | '*')
            && at(i + 1) == Some('\u{FE0F}')
            && at(i + 2) == Some('\u{20E3}')
        {
            i + 3
        } else if is_emoji(c) || (is_text_emoji(c) && at(i + 1) == Some('\u{FE0F}')) {
            let mut j = i + 1;
            loop {
                match at(j) {
                    Some('\u{FE0F}' | '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}')
                    | Some('\u{E0020}'..='\u{E007F}') => j += 1,
                    Some('\u{200D}')
                        if at(j + 1).is_some_and(|next| is_emoji(next) || is_text_emoji(next)) =>
                    {
                        j += 2
                    }
                    _ => break,
                }
            }
            j
        } else {
            i += 1;
            continue;
        };
        found.push(Cluster {
            units: position(i)..position(end),
            first: c.len_utf16(),
        });
        i = end;
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
thread_local! {
    static PAINTER: RefCell<Option<Painter>> = const { RefCell::new(None) };
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
    /// Width, height, and baseline of `text` at `size` pixels.
    fn measure(
        &self,
        text: &[u16],
        format: &IDWriteTextFormat,
    ) -> windows::core::Result<(f32, f32, f32)> {
        // SAFETY: the layout borrows only these live arguments during the calls.
        unsafe {
            let layout = self
                .dwrite
                .CreateTextLayout(text, format, 100_000.0, 100_000.0)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            let mut lines = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0;
            layout.GetLineMetrics(Some(&mut lines), &mut count)?;
            Ok((
                metrics.widthIncludingTrailingWhitespace,
                metrics.height,
                lines[0].baseline,
            ))
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

/// Draws the visible emoji of a just-painted RichEdit window in color over a `paper`
/// background. Returns whether any emoji were visible. Emoji inside the selection keep
/// RichEdit's highlighted glyph.
pub fn paint(hwnd: *mut c_void, paper: u32) -> bool {
    let mut client = SysRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a live window and a writable rectangle.
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 || client.right <= 0 || client.bottom <= 0 {
        return false;
    }
    let length = send(hwnd, WM_GETTEXTLENGTH, 0, 0).max(0) as i32;
    // Back up a little so a sequence that starts above the window is still found whole.
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
    let _ = PAINTER.with(|cell| -> windows::core::Result<()> {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = Some(Painter::new()?);
        }
        let painter = cell.as_ref().expect("painter was just created");
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
        let point = |cp: i32| -> Option<(f32, f32)> {
            // SAFETY: a live document; GetPoint writes the two integers.
            unsafe {
                let range = document.Range(cp, cp).ok()?;
                let (mut x, mut y) = (0, 0);
                range.GetPoint(POINT_FLAGS, &mut x, &mut y).ok()?;
                Some((x as f32, y as f32))
            }
        };
        // SAFETY: the window DC is released below on every path.
        let dc = unsafe { GetDC(hwnd) };
        if dc.is_null() {
            return Ok(());
        }
        let result = (|| -> windows::core::Result<()> {
            let target = &painter.target;
            let bounds = RECT {
                left: 0,
                top: 0,
                right: client.right,
                bottom: client.bottom,
            };
            // SAFETY: the DC and rectangle are valid for this draw.
            unsafe {
                target.BindDC(HDC(dc), &bounds)?;
                let background = target.CreateSolidColorBrush(&color(paper), None)?;
                let dark = ((paper >> 16) & 255) + ((paper >> 8) & 255) + (paper & 255) < 384;
                let ink = target
                    .CreateSolidColorBrush(&color(if dark { 0xe6e6e5 } else { 0x1a1a1a }), None)?;
                target.BeginDraw();
                for cluster in &found {
                    let start = first + cluster.units.start as i32;
                    let end = first + cluster.units.end as i32;
                    if selected.min < selected.max && selected.min < end && start < selected.max {
                        continue;
                    }
                    let (Some((x0, baseline)), Some((x_first, y_first)), Some((x_end, y_end))) = (
                        point(start),
                        point(start + cluster.first as i32),
                        point(end),
                    ) else {
                        continue;
                    };
                    // A sequence broken across lines keeps RichEdit's glyphs.
                    if y_first != baseline || y_end != baseline || x_first <= x0 {
                        continue;
                    }
                    let text = &units[cluster.units.clone()];
                    let first_glyph = &text[..cluster.first];
                    // Size the color glyph so its first code point matches RichEdit's width.
                    let (unit_width, _, _) =
                        painter.measure(first_glyph, &painter.format(100.0)?)?;
                    if unit_width <= 0.0 {
                        continue;
                    }
                    let format = painter.format(100.0 * (x_first - x0) / unit_width)?;
                    let (width, height, ascent) = painter.measure(text, &format)?;
                    let top = baseline - ascent;
                    if top > client.bottom as f32 || top + height < 0.0 {
                        continue;
                    }
                    let right = x_end.max(x0 + width);
                    target.FillRectangle(
                        &D2D_RECT_F {
                            left: x0,
                            top,
                            right,
                            bottom: top + height,
                        },
                        &background,
                    );
                    target.DrawText(
                        text,
                        &format,
                        &D2D_RECT_F {
                            left: x0,
                            top,
                            right: x0 + width + 1.0,
                            bottom: top + height,
                        },
                        &ink,
                        D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                target.EndDraw(None, None)
            }
        })();
        // SAFETY: releases the DC obtained above.
        unsafe {
            ReleaseDC(hwnd, dc);
        }
        result
    });
    true
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
