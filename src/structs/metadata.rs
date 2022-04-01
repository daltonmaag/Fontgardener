use std::collections::BTreeMap;
use std::path::Path;

use super::GlyphRecord;
use norad::Color;
use norad::Name;

pub(crate) fn load_glyph_data(path: &Path) -> BTreeMap<Name, GlyphRecord> {
    let mut glyph_data = BTreeMap::new();
    let mut reader = csv::Reader::from_path(path).expect("can't open glyph_data.csv");

    type Record = (String, Option<String>, Option<String>, Option<String>, bool);
    for result in reader.deserialize() {
        let record: Record = result.expect("can't read record");
        glyph_data.insert(
            Name::new(&record.0).expect("can't read glyph name"),
            GlyphRecord {
                postscript_name: record.1,
                codepoints: record.2.map(|v| parse_codepoints(&v)).unwrap_or_default(),
                opentype_category: record.3,
                export: record.4,
            },
        );
    }

    glyph_data
}

fn parse_codepoints(v: &str) -> Vec<char> {
    v.split_whitespace()
        .map(|v| {
            char::try_from(u32::from_str_radix(v, 16).expect("can't parse codepoint"))
                .expect("can't convert codepoint to character")
        })
        .collect()
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

pub(crate) fn load_color_marks(path: &Path) -> BTreeMap<Name, Color> {
    let mut color_marks = BTreeMap::new();

    if !path.exists() {
        return color_marks;
    }

    let mut reader = csv::Reader::from_path(&path).expect("can't open color_marks.csv");
    for result in reader.deserialize() {
        let record: (Name, Color) = result.expect("can't read color mark");
        color_marks.insert(record.0, record.1);
    }
    color_marks
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
