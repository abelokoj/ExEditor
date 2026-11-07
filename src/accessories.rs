//! Explicitly downloaded font packs, kept in ExEditor's own application-data folder.
use anyhow::{Context as _, Result, ensure};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

pub type FontFiles = Vec<(Vec<u8>, String)>;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub source_commit: String,
    pub packs: Vec<FontPack>,
}
#[derive(Debug, Deserialize)]
pub struct FontPack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub fonts: Vec<Asset>,
    license: Asset,
}
#[derive(Debug, Deserialize)]
pub struct Asset {
    file: String,
    bytes: u64,
    sha256: String,
}

pub fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/accessories/fonts.json"))
            .expect("the compiled-in font accessory manifest must be valid")
    })
}
impl FontPack {
    pub fn download_bytes(&self) -> u64 {
        self.fonts.iter().map(|font| font.bytes).sum::<u64>() + self.license.bytes
    }
}
fn root() -> Result<PathBuf> {
    Ok(crate::document::data_dir()?.join("accessories/fonts"))
}
fn verify(asset: &Asset, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() as u64 == asset.bytes,
        "{} is incomplete or has the wrong size. Retry the download.",
        asset.file
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == asset.sha256,
        "{} failed its integrity check. Retry the download.",
        asset.file
    );
    Ok(())
}
fn read_asset(asset: &Asset, directory: &Path) -> Result<Vec<u8>> {
    let file = File::open(directory.join(&asset.file))?;
    ensure!(
        file.metadata()?.len() == asset.bytes,
        "{} has the wrong size.",
        asset.file
    );
    let mut bytes = Vec::new();
    file.take(asset.bytes + 1).read_to_end(&mut bytes)?;
    verify(asset, &bytes)?;
    Ok(bytes)
}
fn read_pack_at(pack: &FontPack, root: &Path) -> Result<Option<FontFiles>> {
    let directory = root.join(&pack.id);
    if !directory.try_exists()? {
        return Ok(None);
    }
    read_asset(&pack.license, &directory)?;
    let mut files = Vec::with_capacity(pack.fonts.len());
    for asset in &pack.fonts {
        files.push((read_asset(asset, &directory)?, asset.file.clone()));
    }
    Ok(Some(files))
}
pub fn read_pack(pack: &FontPack) -> Result<Option<FontFiles>> {
    read_pack_at(pack, &root()?)
}
fn fetch(agent: &ureq::Agent, pack: &FontPack, asset: &Asset) -> Result<Vec<u8>> {
    let url = format!(
        "https://raw.githubusercontent.com/abelokoj/ExEditor/{}/assets/fonts/{}/{}",
        manifest().source_commit,
        pack.id,
        asset.file
    );
    let mut response = agent.get(&url).call().with_context(|| {
        format!(
            "Downloading {}. Check your internet connection and retry.",
            pack.name
        )
    })?;
    ensure!(
        response.status().as_u16() == 200,
        "GitHub did not return the requested font file."
    );
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(asset.bytes + 1)
        .read_to_end(&mut bytes)?;
    verify(asset, &bytes)?;
    Ok(bytes)
}
fn install_at(pack: &FontPack, fonts: &FontFiles, license: &[u8], root: &Path) -> Result<()> {
    ensure!(
        fonts.len() == pack.fonts.len(),
        "The font pack is incomplete."
    );
    verify(&pack.license, license)?;
    for (asset, (bytes, name)) in pack.fonts.iter().zip(fonts) {
        ensure!(
            *name == asset.file,
            "The downloaded font does not match the selected pack."
        );
        verify(asset, bytes)?;
    }
    let directory = root.join(&pack.id);
    fs::create_dir_all(&directory).context("Creating the font accessory folder")?;
    crate::document::atomic_write(&directory.join(&pack.license.file), license)?;
    for (bytes, name) in fonts {
        crate::document::atomic_write(&directory.join(name), bytes)?;
    }
    Ok(())
}
/// Called only after an explicit Settings download action; startup never uses the network.
pub fn download(pack: &'static FontPack) -> Result<FontFiles> {
    download_to(pack, &root()?)
}
fn download_to(pack: &FontPack, directory: &Path) -> Result<FontFiles> {
    if let Ok(Some(files)) = read_pack_at(pack, directory) {
        return Ok(files);
    }
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut fonts = Vec::with_capacity(pack.fonts.len());
    for asset in &pack.fonts {
        fonts.push((fetch(&agent, pack, asset)?, asset.file.clone()));
    }
    let license = fetch(&agent, pack, &pack.license)?;
    install_at(pack, &fonts, &license, directory)?;
    Ok(fonts)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(pack: &FontPack) -> (FontFiles, Vec<u8>) {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/fonts")
            .join(&pack.id);
        (
            pack.fonts
                .iter()
                .map(|asset| {
                    (
                        fs::read(directory.join(&asset.file)).unwrap(),
                        asset.file.clone(),
                    )
                })
                .collect(),
            fs::read(directory.join(&pack.license.file)).unwrap(),
        )
    }
    #[test]
    fn optional_font_manifest_matches_the_repository_without_embedding_font_bytes() {
        let manifest = manifest();
        assert_eq!(manifest.packs.len(), 3);
        assert_eq!(manifest.source_commit.len(), 40);
        assert!(
            manifest
                .source_commit
                .bytes()
                .all(|ch| ch.is_ascii_hexdigit())
        );
        for pack in &manifest.packs {
            assert!(
                pack.id
                    .bytes()
                    .all(|ch| ch.is_ascii_lowercase() || ch == b'-')
            );
            for asset in pack.fonts.iter().chain(std::iter::once(&pack.license)) {
                assert!(
                    asset
                        .file
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-' | b'_'))
                );
                assert!(!asset.file.contains(".."));
                assert_eq!(asset.sha256.len(), 64);
                assert!(asset.sha256.bytes().all(|ch| ch.is_ascii_hexdigit()));
            }
            let (fonts, license) = fixture(pack);
            verify(&pack.license, &license).unwrap();
            for (asset, (bytes, name)) in pack.fonts.iter().zip(&fonts) {
                assert_eq!(name, &asset.file);
                assert!(asset.bytes <= 16 * 1024 * 1024);
                verify(asset, bytes).unwrap();
                wordcraft_fonts::decode_font_file(bytes).expect("valid optional font");
            }
        }
    }
    #[test]
    #[ignore = "explicit network check run only by the GitHub workflow"]
    fn github_font_downloads_are_verified_and_cached() {
        let root = tempfile::tempdir().unwrap();
        for pack in &manifest().packs {
            let files =
                download_to(pack, root.path()).expect("download from the pinned GitHub source");
            assert_eq!(read_pack_at(pack, root.path()).unwrap().unwrap(), files);
            assert_eq!(
                download_to(pack, root.path()).unwrap(),
                files,
                "reuse verified offline files"
            );
        }
    }
    #[test]
    fn missing_partial_and_corrupt_packs_never_replace_a_valid_installation() {
        let pack = &manifest().packs[0];
        let root = tempfile::tempdir().unwrap();
        assert!(read_pack_at(pack, root.path()).unwrap().is_none());
        let (mut fonts, license) = fixture(pack);
        install_at(pack, &fonts, &license, root.path()).unwrap();
        let before = read_pack_at(pack, root.path()).unwrap().unwrap();
        fonts[0].0[0] ^= 1;
        assert!(install_at(pack, &fonts, &license, root.path()).is_err());
        assert_eq!(read_pack_at(pack, root.path()).unwrap().unwrap(), before);
        let path = root.path().join(&pack.id).join(&pack.fonts[0].file);
        fs::write(&path, &fonts[0].0).unwrap();
        assert!(
            read_pack_at(pack, root.path()).is_err(),
            "same-size corruption must be rejected"
        );
        fs::write(&path, []).unwrap();
        assert!(read_pack_at(pack, root.path()).is_err());
        fs::remove_file(path).unwrap();
        assert!(
            read_pack_at(pack, root.path()).is_err(),
            "partial packs are repairable, not installed"
        );
    }
}
