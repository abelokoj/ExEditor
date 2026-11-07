//! Inter is bundled; additional process-private typefaces are optional Settings downloads.
use crate::accessories::{FontFiles, FontPack};
use anyhow::{Context as _, Result};
use gpui_kit::{App, Global};
use std::{
    borrow::Cow,
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

pub const RICH_FONT: &str = "Pretendard Std";
pub const PLAIN_FONT: &str = "Maple Mono NF";
pub const INTER_FONT: &str = "Inter";
#[cfg(windows)]
pub const SYSTEM_PLAIN_FONT: &str = "Consolas";
#[cfg(target_os = "macos")]
pub const SYSTEM_PLAIN_FONT: &str = "Menlo";
#[cfg(not(any(windows, target_os = "macos")))]
pub const SYSTEM_PLAIN_FONT: &str = "DejaVu Sans Mono";
static AVAILABLE: AtomicU8 = AtomicU8::new(0);
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
];
fn pack_bit(id: &str) -> u8 {
    match id {
        "pretendard-std" => 1,
        "maple-mono-nf" => 2,
        "noto-color-emoji" => 4,
        _ => 0,
    }
}
pub fn accessory_available(id: &str) -> bool {
    AVAILABLE.load(Ordering::Acquire) & pack_bit(id) != 0
}
pub fn default_rich_font() -> &'static str {
    if accessory_available("pretendard-std") {
        RICH_FONT
    } else {
        INTER_FONT
    }
}
pub fn default_plain_font() -> &'static str {
    if accessory_available("maple-mono-nf") {
        return PLAIN_FONT;
    }
    SYSTEM_PLAIN_FONT
}
fn register_engine_pack(pack: &FontPack, files: &FontFiles) -> Result<()> {
    let decoded = files
        .iter()
        .map(|(bytes, name)| {
            Ok((
                wordcraft_fonts::decode_font_file(bytes)
                    .with_context(|| format!("Loading {}", pack.name))?,
                name.clone(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    wordcraft_fonts::FontDb::global().add_document_fonts(decoded);
    AVAILABLE.fetch_or(pack_bit(&pack.id), Ordering::AcqRel);
    Ok(())
}
#[derive(Debug)]
struct RegisteredFonts;
impl Global for RegisteredFonts {}

/// Registers bundled Inter and already-downloaded fonts without any network request.
pub fn register(cx: &mut App) -> Result<()> {
    if cx.has_global::<RegisteredFonts>() {
        return Ok(());
    }
    register_engine_fonts()?;
    let mut fonts: Vec<Cow<'static, [u8]>> = FONTS
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    for pack in &crate::accessories::manifest().packs {
        if accessory_available(&pack.id)
            && pack.id != "noto-color-emoji"
            && let Ok(Some(files)) = crate::accessories::read_pack(pack)
        {
            fonts.extend(files.into_iter().map(|(bytes, _)| Cow::Owned(bytes)));
        }
    }
    cx.text_system()
        .add_fonts(fonts)
        .context("Loading the interface fonts")?;
    cx.set_global(RegisteredFonts);
    Ok(())
}
/// Loads bundled fonts once. Missing or damaged accessories never prevent opening the app.
pub fn register_engine_fonts() -> Result<()> {
    static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let mut files = Vec::with_capacity(FONTS.len());
            for &(name, bytes) in FONTS {
                files.push((
                    wordcraft_fonts::decode_font_file(bytes)
                        .map_err(|error| format!("Loading bundled {name}: {error}"))?,
                    name.to_owned(),
                ));
            }
            wordcraft_fonts::FontDb::global().add_document_fonts(files);
            for pack in &crate::accessories::manifest().packs {
                match crate::accessories::read_pack(pack) {
                    Ok(Some(files)) => {
                        if let Err(error) = register_engine_pack(pack, &files) {
                            eprintln!("Optional font: {error}");
                        }
                    }
                    Ok(None) => {}
                    Err(error) => eprintln!(
                        "Optional {} is unavailable: {error}. Retry from Settings > Accessories.",
                        pack.name
                    ),
                }
            }
            Ok(())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(())
}
/// Activates a verified download immediately, without installing a system font or restarting.
pub fn register_accessory(pack: &FontPack, files: FontFiles, cx: &mut App) -> Result<()> {
    if pack.id != "noto-color-emoji" {
        cx.text_system()
            .add_fonts(
                files
                    .iter()
                    .map(|(bytes, _)| Cow::Owned(bytes.clone()))
                    .collect(),
            )
            .context("Loading the downloaded font into the interface")?;
    }
    register_engine_pack(pack, &files)?;
    cx.refresh_windows();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui_kit::test]
    fn downloaded_fonts_refresh_layout_without_changing_saved_content(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let editor = crate::editor::Editor::new().unwrap();
        editor.set_text("Optional font sample").unwrap();
        editor.mark_saved();
        let snapshot = editor.snapshot();
        let layout = editor.layout();
        let revision = editor.revision();
        let document_revision = editor.document_revision();
        let pack = &crate::accessories::manifest().packs[0];
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/fonts")
            .join(&pack.id);
        let files = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ttf"))
            .map(|path| {
                (
                    std::fs::read(&path).unwrap(),
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        cx.update(|cx| register_accessory(pack, files, cx)).unwrap();
        assert!(accessory_available(&pack.id));
        assert_eq!(default_rich_font(), RICH_FONT);
        if editor.revision() != revision {
            assert!(!std::sync::Arc::ptr_eq(&layout, &editor.layout()));
        }
        assert_eq!(editor.document_revision(), document_revision);
        assert_eq!(editor.snapshot(), snapshot);
        assert!(!editor.is_modified());
    }
}
