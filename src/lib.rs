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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LayerInfo {
    name: Name,
    #[serde(default)]
    default: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to load data from disk")]
    Io(#[from] std::io::Error),
    #[error("a fontgarden must be a directory")]
    NotAFontgarden,
    #[error("cannot import a glyph as it's in a different set already")]
    DuplicateGlyph,
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

    fn save(&self, layer_name: &Name, source_path: &Path) {
        if self.glyphs.is_empty() {
            return;
        }
        let layer_path = source_path.join(format!("glyphs.{layer_name}"));
        std::fs::create_dir(&layer_path).expect("can't create layer dir");

        // TODO: determine default layer
        let layerinfo = LayerInfo {
            name: layer_name.clone(),
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

        write_color_marks(&layer_path.join("color_marks.csv"), &self.color_marks);
    }

    fn from_path(path: &Path) -> Result<(Self, LayerInfo), LoadError> {
        let mut glyphs = HashMap::new();
        let color_marks = load_color_marks(&path.join("color_marks.csv"));
        let layerinfo: LayerInfo =
            plist::from_file(path.join("layerinfo.plist")).expect("can't load layerinfo");

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if path
                    .extension()
                    .and_then(|n| Some(n == "glif"))
                    .unwrap_or(false)
                {
                    let glif = norad::Glyph::load(&path).expect("can't load glif");
                    glyphs.insert(glif.name.clone(), glif);
                }
            }
        }

        Ok((
            Layer {
                glyphs,
                color_marks,
            },
            layerinfo,
        ))
    }
}

fn load_color_marks(path: &Path) -> HashMap<Name, Color> {
    let mut color_marks = HashMap::new();

    if !path.exists() {
        return color_marks;
    }

    let mut rdr = csv::Reader::from_path(&path).expect("can't open color_marks.csv");
    for result in rdr.deserialize() {
        let record: (Name, Color) = result.expect("can't read color mark");
        color_marks.insert(record.0, record.1);
    }
    color_marks
}

fn write_color_marks(path: &Path, color_marks: &HashMap<Name, Color>) {
    let mut wtr = csv::Writer::from_path(&path).expect("can't open color_marks.csv");
    wtr.write_record(&["name", "color"])
        .expect("can't write color_marks header");
    for (name, color) in color_marks {
        wtr.serialize((name, color))
            .expect("can't write color_marks row");
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
        let mut fontgarden = Self::new();
        let mut seen_glyph_names: HashSet<Name> = HashSet::new();

        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = entry.metadata()?;
                if metadata.is_dir() {
                    let name = path
                        .file_name()
                        .expect("can't read filename")
                        .to_string_lossy();
                    if let Some(set_name) = name.strip_prefix("set.") {
                        let set = Set::from_path(&path)?;
                        let coverage = set.glyph_coverage();
                        if !seen_glyph_names
                            .intersection(&coverage)
                            .collect::<Vec<_>>()
                            .is_empty()
                        {
                            return Err(LoadError::DuplicateGlyph);
                        }
                        seen_glyph_names.extend(coverage);
                        fontgarden
                            .sets
                            .insert(Name::new(&set_name).expect("can't read set name"), set);
                    }
                }
            }
        } else {
            return Err(LoadError::NotAFontgarden);
        }

        Ok(fontgarden)
    }

    pub fn import(
        &mut self,
        font: &norad::Font,
        glyphs: &HashSet<String>,
        set_name: &str,
        source_name: &str,
    ) -> Result<(), LoadError> {
        todo!()
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

    fn from_path(path: &std::path::PathBuf) -> Result<Self, LoadError> {
        let glyph_data = load_glyph_data(&path.join("glyph_data.csv"));

        let mut sources = HashMap::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                let name = path
                    .file_name()
                    .expect("can't read filename")
                    .to_string_lossy();
                if let Some(source_name) = name.strip_prefix("source.") {
                    let source = Source::from_path(&path)?;
                    sources.insert(
                        Name::new(source_name).expect("can't read source name"),
                        source,
                    );
                }
            }
        }

        Ok(Set {
            glyph_data,
            sources,
        })
    }

    fn glyph_coverage(&self) -> HashSet<Name> {
        let mut glyphs = HashSet::new();
        glyphs.extend(self.glyph_data.keys().cloned());
        for (_, source) in &self.sources {
            for (_, layer) in &source.layers {
                glyphs.extend(layer.glyphs.keys().cloned());
            }
        }
        glyphs
    }
}

fn load_glyph_data(path: &Path) -> HashMap<Name, GlyphRecord> {
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

    fn from_path(path: &Path) -> Result<Self, LoadError> {
        let mut layers = HashMap::new();

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                if path
                    .file_name()
                    .and_then(|n| Some(n.to_string_lossy().starts_with("glyphs.")))
                    .unwrap_or(false)
                {
                    let (layer, layerinfo) = Layer::from_path(&path)?;
                    layers.insert(layerinfo.name, layer);
                }
            }
        }

        Ok(Source { layers })
    }
}

fn extract_glyph_data(font: &norad::Font, glyphs: &HashSet<&str>) -> HashMap<Name, GlyphRecord> {
    let mut glyph_data: HashMap<Name, GlyphRecord> = HashMap::new();

    let postscript_names = match font.lib.get("public.postscriptNames") {
        Some(v) => v.as_dictionary().unwrap().clone(),
        None => norad::Plist::new(),
    };
    let opentype_categories = match font.lib.get("public.openTypeCategories") {
        Some(v) => v.as_dictionary().unwrap().clone(),
        None => norad::Plist::new(),
    };
    let skip_exports: HashSet<String> = match font.lib.get("public.skipExportGlyphs") {
        Some(v) => v
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_string().unwrap().to_string())
            .collect(),
        None => HashSet::new(),
    };

    for name in glyphs {
        let mut record = GlyphRecord {
            codepoints: font.get_glyph(name).unwrap().codepoints.clone(),
            ..Default::default()
        };
        if let Some(postscript_name) = postscript_names.get(name) {
            record.postscript_name = Some(postscript_name.as_string().unwrap().into());
        }
        if let Some(opentype_category) = opentype_categories.get(name) {
            record.opentype_category = Some(opentype_category.as_string().unwrap().into());
        }
        if skip_exports.contains(*name) {
            record.export = false;
        } else {
            record.export = true;
        }
        glyph_data.insert(Name::new(name).unwrap(), record);
    }

    glyph_data
}

pub(crate) fn load_glyph_list(path: &Path) -> Result<HashSet<String>, std::io::Error> {
    let names: HashSet<String> = std::fs::read_to_string(path)?
        .lines()
        .map(|s| s.trim()) // Remove whitespace for line
        .filter(|s| !s.is_empty()) // Drop now empty lines
        .map(String::from)
        .collect();
    Ok(names)
}

#[cfg(test)]
mod tests {
    // use pretty_assertions::assert_eq;

    use super::*;

    const NOTO_TEMP: &str = r"C:\Users\nikolaus.waxweiler\AppData\Local\Dev\nototest";

    #[test]
    fn load_empty() {
        let tempdir = tempfile::TempDir::new().unwrap();

        let fontgarden = Fontgarden::new();
        fontgarden.save(tempdir.path());
        let fontgarden2 = Fontgarden::from_path(tempdir.path()).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }

    #[test]
    fn it_works() {
        // let tempdir = tempfile::TempDir::new().unwrap();

        let tmp_path = Path::new(NOTO_TEMP);

        let latin_set_name = Name::new("Latin").unwrap();
        let latin_glyphs = HashSet::from(["A", "B", "Adieresis", "Omega"]);

        let ufo_lt = norad::Font::load(tmp_path.join("NotoSans-Light.ufo")).unwrap();
        let source_light_name = Name::new("Light").unwrap();
        let mut source_light_layers: HashMap<Name, Layer> = HashMap::new();
        for layer in ufo_lt.iter_layers() {
            let our_layer = Layer::from_ufo_layer(layer, &latin_glyphs);
            if !our_layer.glyphs.is_empty() {
                source_light_layers.insert(layer.name().clone(), our_layer);
            }
        }
        let source_light = Source {
            layers: source_light_layers,
        };

        let ufo_bd = norad::Font::load(tmp_path.join("NotoSans-Bold.ufo")).unwrap();
        let source_bold_name = Name::new("Bold").unwrap();
        let mut source_bold_layers: HashMap<Name, Layer> = HashMap::new();
        for layer in ufo_bd.iter_layers() {
            let our_layer = Layer::from_ufo_layer(layer, &latin_glyphs);
            if !our_layer.glyphs.is_empty() {
                source_bold_layers.insert(layer.name().clone(), our_layer);
            }
        }
        let source_bold = Source {
            layers: source_bold_layers,
        };

        let latin_glyph_data = extract_glyph_data(&ufo_lt, &latin_glyphs);

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

        // println!("{:#?}", &fontgarden);

        let fg_path = tmp_path.join("test.fontgarden");
        fontgarden.save(&fg_path);
        let fontgarden2 = Fontgarden::from_path(&fg_path).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }
}
