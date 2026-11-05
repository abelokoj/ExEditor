//! Bundled, process-private typefaces shared by GPUI and the native document editor.

use std::{borrow::Cow, ptr};

use anyhow::{Context as _, Result, ensure};
use gpui_kit::{App, Global};
use windows_sys::Win32::Graphics::Gdi::{AddFontMemResourceEx, RemoveFontMemResourceEx};

/// The bundled proportional family used in place of Helvetica.
pub const RICH_FONT: &str = "Pretendard Std";
/// The bundled monospaced family used in place of Menlo.
pub const PLAIN_FONT: &str = "Maple Mono NF";
/// Additional bundled sans-serif family available in the font picker.
pub const INTER_FONT: &str = "Inter";

const FONTS: &[(&str, &[u8])] = &[
    (
        "Inter Regular",
        include_bytes!("../assets/fonts/inter/Inter-Regular.ttf"),
    ),
    (
        "Inter Italic",
        include_bytes!("../assets/fonts/inter/Inter-Italic.ttf"),
    ),
    (
        "Inter Bold",
        include_bytes!("../assets/fonts/inter/Inter-Bold.ttf"),
    ),
    (
        "Inter Bold Italic",
        include_bytes!("../assets/fonts/inter/Inter-BoldItalic.ttf"),
    ),
    (
        "Pretendard Std Regular",
        include_bytes!("../assets/fonts/pretendard-std/PretendardStd-Regular.ttf"),
    ),
    (
        "Pretendard Std Bold",
        include_bytes!("../assets/fonts/pretendard-std/PretendardStd-Bold.ttf"),
    ),
    (
        "Maple Mono NF Regular",
        include_bytes!("../assets/fonts/maple-mono-nf/MapleMono-NF-Regular.ttf"),
    ),
    (
        "Maple Mono NF Italic",
        include_bytes!("../assets/fonts/maple-mono-nf/MapleMono-NF-Italic.ttf"),
    ),
    (
        "Maple Mono NF Bold",
        include_bytes!("../assets/fonts/maple-mono-nf/MapleMono-NF-Bold.ttf"),
    ),
    (
        "Maple Mono NF Bold Italic",
        include_bytes!("../assets/fonts/maple-mono-nf/MapleMono-NF-BoldItalic.ttf"),
    ),
];

#[derive(Debug)]
struct RegisteredFonts(Vec<usize>);

impl Global for RegisteredFonts {}

impl Drop for RegisteredFonts {
    fn drop(&mut self) {
        for handle in self.0.drain(..) {
            // SAFETY: each handle comes from AddFontMemResourceEx and is removed once.
            unsafe { RemoveFontMemResourceEx(handle as _) };
        }
    }
}

/// Loads the bundled fonts into GPUI and this process's Windows text engine.
/// No typeface is installed into the user's system font directory.
///
/// # Errors
/// Returns an error if Windows or GPUI cannot load an embedded font face.
pub fn register(cx: &mut App) -> Result<()> {
    if cx.has_global::<RegisteredFonts>() {
        return Ok(());
    }
    let mut registered = RegisteredFonts(Vec::with_capacity(FONTS.len()));
    for &(face, bytes) in FONTS {
        let length =
            u32::try_from(bytes.len()).context("bundled font exceeds Windows' font size limit")?;
        let mut count = 0;
        // SAFETY: include_bytes provides immutable process-lifetime font data; count is writable.
        let handle =
            unsafe { AddFontMemResourceEx(bytes.as_ptr().cast(), length, ptr::null(), &mut count) };
        ensure!(
            !handle.is_null(),
            "Windows could not load the bundled {face} font"
        );
        registered.0.push(handle as usize);
        ensure!(
            count > 0,
            "Windows found no font faces in the bundled {face} resource"
        );
    }
    let fonts = FONTS
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    cx.text_system()
        .add_fonts(fonts)
        .context("loading the bundled fonts into GPUI")?;
    cx.set_global(registered);
    Ok(())
}
