use std::{
    collections::{HashMap, HashSet},
    path::Path,
    str::FromStr,
};

use norad::{Color, Name};
use serde::{Deserialize, Serialize};

mod metadata;

#[derive(Debug, Default, PartialEq)]
pub struct Fontgarden {
    pub sets: HashMap<Name, Set>,
}

#[derive(Debug, Default, PartialEq)]
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
    pub fn from_ufo_layer(layer: &norad::Layer, glyph_names: &HashSet<Name>) -> Self {
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
        // TODO: keep track of layer file names
        let layer_path = source_path.join(norad::util::default_file_name_for_layer_name(
            &layer_name,
            &HashSet::new(),
        ));
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

        metadata::write_color_marks(&layer_path.join("color_marks.csv"), &self.color_marks);
    }

    fn from_path(path: &Path) -> Result<(Self, LayerInfo), LoadError> {
        let mut glyphs = HashMap::new();
        let color_marks = metadata::load_color_marks(&path.join("color_marks.csv"));
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
        glyphs: &HashSet<Name>,
        set_name: &Name,
        source_name: &Name,
    ) -> Result<(), LoadError> {
        let set = self.sets.entry(set_name.clone()).or_default();
        let source = set.sources.entry(source_name.clone()).or_default();

        // TODO: check for glyph uniqueness per set
        // TODO: follow components and check if they are present in another set
        let glyph_data = extract_glyph_data(font, glyphs);
        set.glyph_data.extend(glyph_data);

        for layer in font.iter_layers() {
            let our_layer = Layer::from_ufo_layer(layer, &glyphs);
            if !our_layer.glyphs.is_empty() {
                source.layers.insert(layer.name().clone(), our_layer);
            }
        }

        Ok(())
    }
}

impl Set {
    pub fn save(&self, set_name: &Name, root_path: &Path) {
        let set_path = root_path.join(format!("set.{set_name}"));
        std::fs::create_dir(&set_path).expect("can't create set dir");

        metadata::write_glyph_data(&self.glyph_data, &set_path.join("glyph_data.csv"));

        for (source_name, source) in &self.sources {
            source.save(source_name, &set_path)
        }
    }

    fn from_path(path: &std::path::PathBuf) -> Result<Self, LoadError> {
        let glyph_data = metadata::load_glyph_data(&path.join("glyph_data.csv"));

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

fn extract_glyph_data(font: &norad::Font, glyphs: &HashSet<Name>) -> HashMap<Name, GlyphRecord> {
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
        if skip_exports.contains(name.as_ref()) {
            record.export = false;
        } else {
            record.export = true;
        }
        glyph_data.insert(name.clone(), record);
    }

    glyph_data
}

pub(crate) fn load_glyph_list(path: &Path) -> Result<HashSet<Name>, std::io::Error> {
    let names: HashSet<Name> = std::fs::read_to_string(path)?
        .lines()
        .map(|s| s.trim()) // Remove whitespace for line
        .filter(|s| !s.is_empty()) // Drop now empty lines
        .map(|v| Name::new(v).unwrap())
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
    fn roundtrip() {
        let mut fontgarden = Fontgarden::new();

        let tmp_path = Path::new(NOTO_TEMP);

        let latin_glyphs: HashSet<Name> = HashSet::from(
            ["A", "B", "Adieresis", "dieresiscomb", "dieresis"].map(|v| Name::new(&v).unwrap()),
        );
        let latin_set_name = Name::new("Latin").unwrap();

        let ufo1 = norad::Font::load(tmp_path.join("NotoSans-Light.ufo")).unwrap();
        let ufo2 = norad::Font::load(tmp_path.join("NotoSans-Bold.ufo")).unwrap();

        fontgarden
            .import(
                &ufo1,
                &latin_glyphs,
                &latin_set_name,
                &Name::new("Light").unwrap(),
            )
            .unwrap();
        fontgarden
            .import(
                &ufo2,
                &latin_glyphs,
                &latin_set_name,
                &Name::new("Bold").unwrap(),
            )
            .unwrap();

        let greek_glyphs: HashSet<Name> = HashSet::from(
            ["Alpha", "Alphatonos", "tonos.case", "tonos"].map(|v| Name::new(&v).unwrap()),
        );
        let greek_set_name = Name::new("Greek").unwrap();

        fontgarden
            .import(
                &ufo1,
                &greek_glyphs,
                &greek_set_name,
                &Name::new("Light").unwrap(),
            )
            .unwrap();
        fontgarden
            .import(
                &ufo2,
                &greek_glyphs,
                &greek_set_name,
                &Name::new("Bold").unwrap(),
            )
            .unwrap();

        let tempdir = tempfile::TempDir::new().unwrap();
        let fg_path = tempdir.path().join("test2.fontgarden");
        fontgarden.save(&fg_path);
        let fontgarden2 = Fontgarden::from_path(&fg_path).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }

    #[test]
    fn roundtrip_big() {
        let tmp_path = Path::new(NOTO_TEMP);
        let mut fontgarden = Fontgarden::new();

        let ufo_paths = [
            "NotoSans-Bold.ufo",
            "NotoSans-Condensed.ufo",
            "NotoSans-CondensedBold.ufo",
            "NotoSans-CondensedLight.ufo",
            "NotoSans-CondensedSemiBold.ufo",
            "NotoSans-DisplayBold.ufo",
            "NotoSans-DisplayBoldCondensed.ufo",
            "NotoSans-DisplayCondensed.ufo",
            "NotoSans-DisplayLight.ufo",
            "NotoSans-DisplayLightCondensed.ufo",
            "NotoSans-DisplayRegular.ufo",
            "NotoSans-DisplaySemiBold.ufo",
            "NotoSans-DisplaySemiBoldCondensed.ufo",
            "NotoSans-Light.ufo",
            "NotoSans-Regular.ufo",
            "NotoSans-SemiBold.ufo",
        ];

        for ufo_path in ufo_paths {
            let font = norad::Font::load(tmp_path.join(ufo_path)).unwrap();
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(&v).unwrap())
                .unwrap();
            let mut ufo_glyph_names: HashSet<Name> = font.iter_names().collect();

            for set_path in ["Latin.txt", "Cyrillic.txt", "Greek.txt"] {
                let set_name = Name::new(set_path.splitn(1, ".").next().unwrap()).unwrap();
                let set_list = load_glyph_list(&tmp_path.join(set_path)).unwrap();

                fontgarden
                    .import(&font, &set_list, &set_name, &source_name)
                    .unwrap();
                ufo_glyph_names.retain(|n| !set_list.contains(n));
            }

            // Put remaining glyphs into default set.
            if !ufo_glyph_names.is_empty() {
                let set_name = Name::new("default").unwrap();
                fontgarden
                    .import(&font, &ufo_glyph_names, &set_name, &source_name)
                    .unwrap();
            }
        }

        // let tempdir = tempfile::TempDir::new().unwrap();
        // let fg_path = tempdir.path().join("test3.fontgarden");
        let fg_path = tmp_path.join("test3.fontgarden");
        fontgarden.save(&fg_path);
        let fontgarden2 = Fontgarden::from_path(&fg_path).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }
}
