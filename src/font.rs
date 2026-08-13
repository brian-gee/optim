use std::path::PathBuf;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontSetBuilder1,
};

/// Iosevka Regular (SIL OFL 1.1 — see LICENSE-IOSEVKA.md), bundled so the
/// default look needs no installed fonts.
static IOSEVKA_TTF: &[u8] = include_bytes!("../assets/iosevka-regular.ttf");

pub const IOSEVKA_FAMILY: &str = "Iosevka";

/// DirectWrite loads collections from files, not memory, so materialize the
/// embedded TTF once under %LOCALAPPDATA%\optim\fonts.
fn ensure_font_file() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?)
        .join("optim")
        .join("fonts")
        .join("iosevka-regular.ttf");
    let current = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if current != IOSEVKA_TTF.len() as u64 {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, IOSEVKA_TTF).ok()?;
    }
    Some(path)
}

/// Builds a private font collection containing the bundled Iosevka.
/// Returns None on any failure; callers fall back to system fonts.
pub fn iosevka_collection(dwrite: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    unsafe {
        let path = ensure_font_file()?;
        let f5: IDWriteFactory5 = dwrite.cast().ok()?;
        let builder: IDWriteFontSetBuilder1 = f5.CreateFontSetBuilder().ok()?;
        let path16: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let file = f5
            .CreateFontFileReference(PCWSTR(path16.as_ptr()), None)
            .ok()?;
        builder.AddFontFile(&file).ok()?;
        let set = builder.CreateFontSet().ok()?;
        let collection = f5.CreateFontCollectionFromFontSet(&set).ok()?;
        collection.cast().ok()
    }
}
