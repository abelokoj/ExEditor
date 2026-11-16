//! Process-private fonts for the native editor and the separate PDF codec.
use crate::accessories::{FontFiles, FontPack};
use anyhow::{Context as _, Result};
use nsh_core::element::Root;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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
static ACCESSORIES_REGISTERED: AtomicBool = AtomicBool::new(false);
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/inter/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-BoldItalic.ttf"),
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
        PLAIN_FONT
    } else {
        SYSTEM_PLAIN_FONT
    }
}

/// Registers bundled Inter with the native text system before the first render.
pub fn configure_root(root: Root) -> Root {
    FONTS.iter().fold(root, |root, bytes| root.font(*bytes))
}

/// Activates downloaded native font accessories without registering Inter again.
pub async fn register() -> Result<()> {
    if ACCESSORIES_REGISTERED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    for pack in &crate::accessories::manifest().packs {
        match crate::accessories::read_pack(pack) {
            Ok(Some(files)) => {
                if let Err(error) = register_accessory(pack, files).await {
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
}

/// Activates verified accessory data without a system installation or restart.
pub async fn register_accessory(pack: &FontPack, files: FontFiles) -> Result<()> {
    for (bytes, _) in files {
        nsh_core::element::register_font(bytes)
            .await
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("Loading {}", pack.name))?;
    }
    AVAILABLE.fetch_or(pack_bit(&pack.id), Ordering::AcqRel);
    Ok(())
}

/// Provides verified, process-private fonts to the independent print worker.
pub fn export_font_data() -> Result<Vec<Vec<u8>>> {
    let mut fonts: Vec<Vec<u8>> = FONTS.iter().map(|bytes| bytes.to_vec()).collect();
    for pack in &crate::accessories::manifest().packs {
        if let Some(files) = crate::accessories::read_pack(pack)
            .with_context(|| format!("Verifying {} for PDF output", pack.name))?
        {
            fonts.extend(files.into_iter().map(|(bytes, _)| bytes));
        }
    }
    Ok(fonts)
}
