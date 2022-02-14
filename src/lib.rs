use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
    str::FromStr,
};

use norad::{Color, Name};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Fontgarden {
    pub sets: HashMap<Name, Set>,
}

#[derive(Debug)]
pub struct Set {
    pub glyph_data: HashMap<Name, GlyphRecord>,
    pub sources: HashMap<Name, Source>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GlyphRecord {
    pub postscript_name: Option<String>,
    #[serde(default)]
    pub codepoints: Vec<char>,
    // TODO: Make an enum
    pub opentype_category: Option<String>,
    // TODO: Write fn default that sets true here
    #[serde(default = "default_true")]
    pub export: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default)]
pub struct Source {
    pub layers: HashMap<Name, Layer>,
}

#[derive(Debug)]
pub struct Layer {
    pub glyphs: HashMap<Name, norad::Glyph>,
    pub color_marks: HashMap<Name, norad::Color>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LayerInfo {
    name: String,
    #[serde(default)]
    default: bool,
}

impl Layer {
    pub fn from_ufo_layer(layer: &norad::Layer, glyph_names: &HashSet<&str>) -> Self {
        let mut glyphs = HashMap::new();
        let mut color_marks = HashMap::new();

        for glyph in layer
            .iter()
            .filter(|g| glyph_names.contains(g.name.as_str()))
        {
            let mut our_glyph = glyph.clone();
            if let Some(color_string) = our_glyph.lib.remove("public.markColor") {
                let our_color = Color::from_str(color_string.as_string().unwrap()).unwrap();
                color_marks.insert(glyph.name.clone(), our_color);
            }
            // TODO: split out the codepoints.
            glyphs.insert(glyph.name.clone(), our_glyph);
        }

        Self {
            glyphs,
            color_marks,
        }
    }

    fn save(&self, layer_name: &str, source_path: &Path) {
        if self.glyphs.is_empty() {
            return;
        }
        let layer_path = source_path.join(format!("glyphs.{layer_name}"));
        std::fs::create_dir(&layer_path).expect("can't create layer dir");

        // TODO: determine default layer
        let layerinfo = LayerInfo {
            name: layer_name.into(),
            default: false,
        };
        plist::to_file_xml(layer_path.join("layerinfo.plist"), &layerinfo)
            .expect("can't write layerinfo");

        let mut filenames = HashSet::new();
        for (glyph_name, glyph) in &self.glyphs {
            let filename = norad::util::default_file_name_for_glyph_name(glyph_name, &filenames);
            let glyph_path = layer_path.join(&filename);
            glyph.save(&glyph_path).expect("can't write glif file");
            filenames.insert(filename.to_string_lossy().to_string());
        }
    }
}

impl Fontgarden {
    pub fn save(&self, path: &Path) {
        if path.exists() {
            std::fs::remove_dir_all(path).expect("can't remove target dir");
        }
        std::fs::create_dir(path).expect("can't create target dir");

        for (set_name, set) in &self.sets {
            set.save(set_name, path);
        }
    }
}

impl Set {
    pub fn save(&self, set_name: &Name, root_path: &Path) {
        let set_path = root_path.join(format!("set.{set_name}"));
        std::fs::create_dir(&set_path).expect("can't create set dir");

        self.write_glyph_data(&set_path);

        for (source_name, source) in &self.sources {
            source.save(source_name, &set_path)
        }
    }

    fn write_glyph_data(&self, set_path: &Path) {
        let glyph_data_csv_file =
            File::create(&set_path.join("glyph_data.csv")).expect("can't create glyph_data.csv");
        let mut glyph_data_keys: Vec<_> = self.glyph_data.keys().collect();
        glyph_data_keys.sort();
        let mut writer = csv::Writer::from_writer(glyph_data_csv_file);
        GlyphRecord::write_header_to_csv(&mut writer).expect("can't write csv");
        for glyph_name in glyph_data_keys {
            let record = &self.glyph_data[glyph_name];
            record
                .write_row_to_csv(glyph_name, &mut writer)
                .expect("can't write record");
        }
        writer.flush().expect("can't flush csv");
    }
}

impl GlyphRecord {
    pub fn write_header_to_csv(
        writer: &mut csv::Writer<File>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        writer.write_record(&[
            "name",
            "postscript_name",
            "codepoints",
            "opentype_category",
            "export",
        ])?;

        Ok(())
    }

    pub fn write_row_to_csv(
        &self,
        glyph_name: &Name,
        writer: &mut csv::Writer<File>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let codepoints_str: String = self
            .codepoints
            .iter()
            .map(|c| format!("{:04X}", *c as usize))
            .collect::<Vec<_>>()
            .join(" ");

        writer.serialize((
            glyph_name,
            &self.postscript_name,
            codepoints_str,
            &self.opentype_category,
            self.export,
        ))?;

        Ok(())
    }
}

impl Source {
    fn save(&self, source_name: &str, set_path: &Path) {
        let source_path = set_path.join(format!("source.{source_name}"));
        std::fs::create_dir(&source_path).expect("can't create source dir");
        for (layer_name, layer) in &self.layers {
            layer.save(layer_name, &source_path);
        }
    }
}
