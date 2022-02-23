use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use super::GlyphRecord;
use norad::Color;
use norad::Name;

pub(crate) fn load_glyph_data(path: &Path) -> HashMap<Name, GlyphRecord> {
    let mut glyph_data = HashMap::new();
    let mut reader = csv::Reader::from_path(path).expect("can't open glyph_data.csv");

    type Record = (String, Option<String>, Option<String>, Option<String>, bool);
    for result in reader.deserialize() {
        let record: Record = result.expect("can't read record");
        glyph_data.insert(
            Name::new(&record.0).expect("can't read glyph name"),
            GlyphRecord {
                postscript_name: record.1,
                codepoints: record.2.map(|v| parse_codepoints(&v)).unwrap_or(Vec::new()),
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

pub(crate) fn write_glyph_data(glyph_data: &HashMap<Name, GlyphRecord>, path: &Path) {
    let glyph_data_csv_file = File::create(&path).expect("can't create glyph_data.csv");
    let mut glyph_data_keys: Vec<_> = glyph_data.keys().collect();
    glyph_data_keys.sort();

    let mut writer = csv::Writer::from_writer(glyph_data_csv_file);
    writer
        .write_record(&[
            "name",
            "postscript_name",
            "codepoints",
            "opentype_category",
            "export",
        ])
        .expect("can't write csv");
    for glyph_name in glyph_data_keys {
        let record = &glyph_data[glyph_name];
        let codepoints_str: String = record
            .codepoints
            .iter()
            .map(|c| format!("{:04X}", *c as usize))
            .collect::<Vec<_>>()
            .join(" ");
        writer
            .serialize((
                glyph_name,
                &record.postscript_name,
                codepoints_str,
                &record.opentype_category,
                record.export,
            ))
            .expect("can't write csv row");
    }
    writer.flush().expect("can't flush csv");
}

pub(crate) fn load_color_marks(path: &Path) -> HashMap<Name, Color> {
    let mut color_marks = HashMap::new();

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

pub(crate) fn write_color_marks(path: &Path, color_marks: &HashMap<Name, Color>) {
    let mut writer = csv::Writer::from_path(&path).expect("can't open color_marks.csv");
    writer
        .write_record(&["name", "color"])
        .expect("can't write color_marks header");
    for (name, color) in color_marks {
        writer
            .serialize((name, color))
            .expect("can't write color_marks row");
    }
}
