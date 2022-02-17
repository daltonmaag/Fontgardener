use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
    str::FromStr,
};

use norad::{Color, Name};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq)]
pub struct Fontgarden {
    pub sets: HashMap<Name, Set>,
}

#[derive(Debug, PartialEq)]
pub struct Set {
    pub glyph_data: HashMap<Name, GlyphRecord>,
    pub sources: HashMap<Name, Source>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Default, PartialEq)]
pub struct Source {
    pub layers: HashMap<Name, Layer>,
}

#[derive(Debug, PartialEq)]
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

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to load data from disk")]
    Io(#[from] std::io::Error),
    #[error("a fontgarden must be a directory")]
    NotAFontgarden,
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

    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_path(path: &Path) -> Result<Self, LoadError> {
        let fontgarden = Self::new();

        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    if path
                        .file_name()
                        .map(|n| n.to_string_lossy().starts_with("set."))
                        .unwrap_or(false)
                    {
                        todo!();
                    }
                }
            }
        } else {
            return Err(LoadError::NotAFontgarden);
        }

        Ok(fontgarden)
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

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO_TEMP: &str = r"C:\Users\nikolaus.waxweiler\AppData\Local\Dev\nototest";

    #[test]
    fn it_works() {
        let tmp_path = Path::new(NOTO_TEMP);

        let latin_set_name = Name::new("Latin").unwrap();
        let latin_glyphs = HashSet::from(["A", "B", "Adieresis", "Omega"]);

        let ufo_lt = norad::Font::load(tmp_path.join("NotoSans-Light.ufo")).unwrap();
        let source_light_name = Name::new("Light").unwrap();
        let mut source_light = Source::default();
        for layer in ufo_lt.iter_layers() {
            let our_layer = Layer::from_ufo_layer(layer, &latin_glyphs);
            source_light.layers.insert(layer.name().clone(), our_layer);
        }

        let ufo_bd = norad::Font::load(tmp_path.join("NotoSans-Bold.ufo")).unwrap();
        let source_bold_name = Name::new("Bold").unwrap();
        let mut source_bold = Source::default();
        for layer in ufo_bd.iter_layers() {
            let our_layer = Layer::from_ufo_layer(layer, &latin_glyphs);
            source_bold.layers.insert(layer.name().clone(), our_layer);
        }

        let postscript_names = match ufo_lt.lib.get("public.postscriptNames") {
            Some(v) => v.as_dictionary().unwrap().clone(),
            None => norad::Plist::new(),
        };
        let opentype_categories = match ufo_lt.lib.get("public.openTypeCategories") {
            Some(v) => v.as_dictionary().unwrap().clone(),
            None => norad::Plist::new(),
        };
        let skip_exports: HashSet<String> = match ufo_lt.lib.get("public.skipExportGlyphs") {
            Some(v) => v
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_string().unwrap().to_string())
                .collect(),
            None => HashSet::new(),
        };

        let mut latin_glyph_data: HashMap<Name, GlyphRecord> = HashMap::new();
        for name in latin_glyphs {
            let mut record = GlyphRecord {
                codepoints: ufo_lt.get_glyph(name).unwrap().codepoints.clone(),
                ..Default::default()
            };
            if let Some(postscript_name) = postscript_names.get(name) {
                record.postscript_name = Some(postscript_name.as_string().unwrap().into());
            }
            if let Some(opentype_category) = opentype_categories.get(name) {
                record.opentype_category = Some(opentype_category.as_string().unwrap().into());
            }
            if skip_exports.contains(name) {
                record.export = false;
            } else {
                record.export = true;
            }
            latin_glyph_data.insert(Name::new(name).unwrap(), record);
        }

        let mut sources = HashMap::new();
        sources.insert(source_light_name, source_light);
        sources.insert(source_bold_name, source_bold);
        let latin_set = Set {
            glyph_data: latin_glyph_data,
            sources,
        };

        let mut sets = HashMap::new();
        sets.insert(latin_set_name, latin_set);
        let fontgarden = Fontgarden { sets };

        println!("{:#?}", &fontgarden);

        fontgarden.save(&tmp_path.join("test.fontgarden"));
    }

    #[test]
    fn load_empty() {
        let tempdir = tempfile::TempDir::new().unwrap();

        let fontgarden = Fontgarden::new();
        fontgarden.save(tempdir.path());
        let fontgarden2 = Fontgarden::from_path(tempdir.path()).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }
}
