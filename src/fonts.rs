//! Bundled, process-private typefaces shared by GPUI and the native document editor.

use anyhow::{Context as _, Result};
use gpui_kit::{App, Global};
use std::{borrow::Cow, sync::OnceLock};

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
    (
        "Noto Color Emoji",
        include_bytes!("../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf"),
    ),
];

#[derive(Debug)]
struct RegisteredFonts;
impl Global for RegisteredFonts {}

/// Loads bundled fonts into GPUI and WordCraft for this process.
/// No typeface is installed into the user's system font directory.
///
/// # Errors
/// Returns an error if GPUI or WordCraft cannot load an embedded font face.
pub fn register(cx: &mut App) -> Result<()> {
    if cx.has_global::<RegisteredFonts>() {
        return Ok(());
    }
    register_engine_fonts()?;
    let fonts = FONTS
        .iter()
        .filter(|(name, _)| *name != "Noto Color Emoji")
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    cx.text_system()
        .add_fonts(fonts)
        .context("loading the bundled fonts into GPUI")?;
    cx.set_global(RegisteredFonts);
    Ok(())
}

/// Registers every bundled face in WordCraft without installing system fonts.
/// Pretendard italics use the engine's synthetic oblique, matching the document style.
pub fn register_engine_fonts() -> Result<()> {
    static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let db = wordcraft_fonts::FontDb::global();
            let mut files = Vec::with_capacity(FONTS.len());
            for &(name, bytes) in FONTS {
                let decoded = wordcraft_fonts::decode_font_file(bytes)
                    .map_err(|error| format!("loading bundled {name}: {error}"))?;
                files.push((decoded, name.to_owned()));
            }
            db.add_document_fonts(files);
            Ok(())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(())
}
