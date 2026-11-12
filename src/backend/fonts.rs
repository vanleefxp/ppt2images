//! Font discovery for the pure-Rust renderer.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use betteroffice_pptx::Presentation;
use fontdb::{Database, Family, Query, Stretch, Style, Weight};

/// Family name used for every registered native-renderer face. The renderer
/// falls back to the first registered family for any family the presentation
/// requests that is not installed.
const FALLBACK_FAMILY: &str = "PPT2Images Fallback";

const PREFERRED_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Droid Sans Fallback",
    "WenQuanYi Micro Hei",
    "Microsoft YaHei",
    "PingFang SC",
    "Segoe UI",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
];

/// Registers a Unicode-capable font family with the native renderer.
///
/// Set `PPT2IMAGES_NATIVE_FONT` to a font file to override system discovery.
/// TrueType collections are accepted; the selected face is extracted from the
/// collection before registration.
pub fn register_system_fonts(presentation: &mut Presentation) -> Result<()> {
    if let Some(path) = std::env::var_os("PPT2IMAGES_NATIVE_FONT") {
        let path = Path::new(&path);
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read native font '{}'", path.display()))?;
        let face = extract_face(&bytes, 0).with_context(|| {
            format!(
                "failed to read a usable face from native font '{}'",
                path.display()
            )
        })?;
        register_faces(presentation, &face, None)?;
        return Ok(());
    }

    let database = load_database();
    let regular_face = query_face(&database, Weight::NORMAL).context(
        "no usable system font was found for native rendering; set PPT2IMAGES_NATIVE_FONT to a .ttf or .otf file",
    )?;
    let bold_face = query_face(&database, Weight::BOLD).unwrap_or(regular_face);
    let regular = face_bytes(&database, regular_face)
        .context("failed to read the selected regular system font")?;
    let bold = if bold_face == regular_face {
        None
    } else {
        face_bytes(&database, bold_face)
    };
    register_faces(presentation, &regular, bold.as_deref())?;
    Ok(())
}

fn load_database() -> Database {
    let mut database = Database::new();
    database.load_system_fonts();
    database
}

fn query_face(database: &Database, weight: Weight) -> Option<fontdb::ID> {
    let families: Vec<Family<'_>> = PREFERRED_FAMILIES
        .iter()
        .map(|family| Family::Name(family))
        .collect();
    let query = |families: &[Family<'_>]| {
        database.query(&Query {
            families,
            weight,
            stretch: Stretch::Normal,
            style: Style::Normal,
        })
    };
    query(&families).or_else(|| query(&[Family::SansSerif]))
}

fn face_bytes(database: &Database, id: fontdb::ID) -> Option<Vec<u8>> {
    database.with_face_data(id, extract_face).flatten()
}

fn register_faces(
    presentation: &mut Presentation,
    regular: &[u8],
    bold: Option<&[u8]>,
) -> Result<()> {
    presentation
        .register_font(FALLBACK_FAMILY, false, false, regular)
        .context("failed to register the native fallback font")?;
    if let Some(bold) = bold.filter(|bold| *bold != regular) {
        presentation
            .register_font(FALLBACK_FAMILY, true, false, bold)
            .context("failed to register the native bold fallback font")?;
    }
    Ok(())
}

/// Returns a standalone sfnt font. Font collections are split so the renderer
/// receives the selected face rather than the collection header it cannot
/// parse.
fn extract_face(data: &[u8], face_index: u32) -> Option<Vec<u8>> {
    if !data.starts_with(b"ttcf") {
        return Some(data.to_vec());
    }
    let font_count = read_u32(data, 8)?;
    let face_index = usize::try_from(face_index).ok()?;
    if face_index >= usize::try_from(font_count).ok()? {
        return None;
    }
    let offset_position = 12usize.checked_add(face_index.checked_mul(4)?)?;
    let face_offset = usize::try_from(read_u32(data, offset_position)?).ok()?;
    let table_count = usize::from(read_u16(data, face_offset.checked_add(4)?)?);
    if table_count == 0 || table_count > 4_096 {
        return None;
    }
    let header_length = 12usize.checked_add(table_count.checked_mul(16)?)?;
    let mut output = vec![0u8; header_length];
    output[..12].copy_from_slice(data.get(face_offset..face_offset + 12)?);

    let mut output_offset = header_length;
    for table in 0..table_count {
        let record_offset = face_offset
            .checked_add(12)?
            .checked_add(table.checked_mul(16)?)?;
        let record = data.get(record_offset..record_offset + 16)?;
        let table_offset = usize::try_from(read_u32(record, 8)?).ok()?;
        let table_length = usize::try_from(read_u32(record, 12)?).ok()?;
        let table_data = data.get(table_offset..table_offset.checked_add(table_length)?)?;

        let record_output = 12 + table * 16;
        output[record_output..record_output + 8].copy_from_slice(&record[..8]);
        let output_length = u32::try_from(table_length).ok()?;
        output[record_output + 8..record_output + 12]
            .copy_from_slice(&u32::try_from(output_offset).ok()?.to_be_bytes());
        output[record_output + 12..record_output + 16]
            .copy_from_slice(&output_length.to_be_bytes());

        output.extend_from_slice(table_data);
        while output.len() % 4 != 0 {
            output.push(0);
        }
        output_offset = output.len();
    }
    Some(output)
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::extract_face;

    fn synthetic_collection() -> Vec<u8> {
        let mut collection = Vec::new();
        collection.extend_from_slice(b"ttcf");
        collection.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        collection.extend_from_slice(&1u32.to_be_bytes());
        collection.extend_from_slice(&16u32.to_be_bytes());
        collection.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        collection.extend_from_slice(&1u16.to_be_bytes());
        collection.extend_from_slice(&0u16.to_be_bytes());
        collection.extend_from_slice(&0u16.to_be_bytes());
        collection.extend_from_slice(&0u16.to_be_bytes());
        collection.extend_from_slice(b"head");
        collection.extend_from_slice(&0u32.to_be_bytes());
        collection.extend_from_slice(&44u32.to_be_bytes());
        collection.extend_from_slice(&4u32.to_be_bytes());
        collection.extend_from_slice(&[1, 2, 3, 4]);
        collection
    }

    #[test]
    fn passes_through_standalone_fonts() {
        let font = b"\x00\x01\x00\x00test";
        assert_eq!(extract_face(font, 0).unwrap(), font);
    }

    #[test]
    fn extracts_a_collection_face() {
        let extracted = extract_face(&synthetic_collection(), 0).unwrap();
        assert_eq!(&extracted[..4], b"\x00\x01\x00\x00");
        assert_eq!(&extracted[12..16], b"head");
        assert_eq!(&extracted[20..24], &28u32.to_be_bytes());
        assert_eq!(&extracted[24..28], &4u32.to_be_bytes());
        assert_eq!(&extracted[28..32], &[1, 2, 3, 4]);
    }

    #[test]
    fn rejects_out_of_range_collection_faces() {
        let collection = synthetic_collection();
        assert!(extract_face(&collection, 1).is_none());
    }
}
