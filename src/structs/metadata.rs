use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;

use crate::errors::LoadGlyphDataError;

use super::GlyphRecord;
use norad::Color;
use norad::Name;

pub(crate) fn load_glyph_data(
    path: &Path,
) -> Result<BTreeMap<Name, GlyphRecord>, LoadGlyphDataError> {
    let mut glyph_data = BTreeMap::new();
    let mut reader = csv::Reader::from_path(path).map_err(LoadGlyphDataError::Csv)?;

    type Record = (String, Option<String>, Option<String>, Option<String>, bool);
    for result in reader.deserialize() {
        let record: Record = result.map_err(LoadGlyphDataError::Csv)?;

        let glyph_name =
            Name::new(&record.0).map_err(|e| LoadGlyphDataError::InvalidGlyphName(record.0, e))?;
        let codepoints = match &record.2 {
            Some(codepoints_string) => parse_codepoints(codepoints_string).map_err(|e| {
                LoadGlyphDataError::InvalidCodepoint(
                    glyph_name.clone(),
                    codepoints_string.clone(),
                    e,
                )
            })?,
            None => Vec::new(),
        };

        glyph_data.insert(
            glyph_name,
            GlyphRecord {
                postscript_name: record.1,
                codepoints,
                opentype_category: record.3,
                export: record.4,
            },
        );
    }

    Ok(glyph_data)
}

// NOTE: Use anyhow::Error here because we use anyhow's Context trait in main.
// Something about Sync and Send.
fn parse_codepoints(v: &str) -> Result<Vec<char>, anyhow::Error> {
    let mut codepoints = Vec::new();
    let mut seen = HashSet::new();

    for codepoint in v.split_whitespace() {
        let codepoint = u32::from_str_radix(codepoint, 16)?;
        let codepoint = char::try_from(codepoint)?;
        if seen.insert(codepoint) {
            codepoints.push(codepoint);
        }
    }

    Ok(codepoints)
}

pub(crate) fn write_glyph_data(
    glyph_data: &BTreeMap<Name, GlyphRecord>,
    path: &Path,
) -> Result<(), csv::Error> {
    let mut writer = csv::Writer::from_path(&path)?;

    writer.write_record(&[
        "name",
        "postscript_name",
        "codepoints",
        "opentype_category",
        "export",
    ])?;

    for glyph_name in glyph_data.keys() {
        let record = &glyph_data[glyph_name];
        let codepoints_str: String = record
            .codepoints
            .iter()
            .map(|c| format!("{:04X}", *c as usize))
            .collect::<Vec<_>>()
            .join(" ");
        writer.serialize((
            glyph_name,
            &record.postscript_name,
            codepoints_str,
            &record.opentype_category,
            record.export,
        ))?;
    }
    writer.flush()?;

    Ok(())
}

pub(crate) fn load_color_marks(path: &Path) -> Result<BTreeMap<Name, Color>, csv::Error> {
    let mut color_marks = BTreeMap::new();

    if !path.exists() {
        return Ok(color_marks);
    }

    let mut reader = csv::Reader::from_path(&path)?;
    for result in reader.deserialize() {
        let record: (Name, Color) = result?;
        color_marks.insert(record.0, record.1);
    }

    Ok(color_marks)
}

pub(crate) fn write_color_marks(
    path: &Path,
    color_marks: &BTreeMap<Name, Color>,
) -> Result<(), csv::Error> {
    let mut writer = csv::Writer::from_path(&path)?;

    writer.write_record(&["name", "color"])?;
    for (name, color) in color_marks {
        writer.serialize((name, color))?;
    }
    writer.flush()?;

    Ok(())
}
