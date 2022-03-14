use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use norad::Name;
use serde::{Deserialize, Serialize};

use layer::Layer;
use source::Source;

mod layer;
mod metadata;
mod source;

/// The top-level Fontgarden structure.
///
/// Note: BTreeMaps are used just to make testing easier, as they are ordered
/// and will output a deterministic debug string for textual diffing.
#[derive(Debug, Default, PartialEq)]
pub struct Fontgarden {
    pub sets: BTreeMap<Name, Set>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Set {
    pub glyph_data: BTreeMap<Name, GlyphRecord>,
    pub sources: BTreeMap<Name, Source>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GlyphRecord {
    pub postscript_name: Option<String>,
    #[serde(default)]
    pub codepoints: Vec<char>,
    // TODO: Make an enum
    pub opentype_category: Option<String>,
    #[serde(default = "default_true")]
    pub export: bool,
}

fn default_true() -> bool {
    true
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to load data from disk")]
    Io(#[from] std::io::Error),
    #[error("a fontgarden must be a directory")]
    NotAFontgarden,
    #[error("cannot import a glyph as it's in a different set already")]
    DuplicateGlyph,
    #[error("no default layer for source found")]
    NoDefaultLayer,
}

#[derive(thiserror::Error, Debug)]
pub enum ExportError {
    #[error("failed to load data from disk")]
    Other(#[from] Box<dyn std::error::Error>),
}

impl Fontgarden {
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
                        if seen_glyph_names.intersection(&coverage).next().is_some() {
                            return Err(LoadError::DuplicateGlyph);
                        }
                        seen_glyph_names.extend(coverage);
                        fontgarden
                            .sets
                            .insert(Name::new(set_name).expect("can't read set name"), set);
                    }
                }
            }
        } else {
            return Err(LoadError::NotAFontgarden);
        }

        Ok(fontgarden)
    }

    pub fn save(&self, path: &Path) {
        if path.exists() {
            std::fs::remove_dir_all(path).expect("can't remove target dir");
        }
        std::fs::create_dir(path).expect("can't create target dir");

        for (set_name, set) in &self.sets {
            set.save(set_name, path);
        }
    }

    /// Import glyphs from a UFO into the Fontgarden.
    ///
    /// Strategy: for each imported glyph, if the name already exists in some
    /// set, import it there, else import it into `set_name`.
    pub fn import(
        &mut self,
        font: &norad::Font,
        glyphs: &HashSet<Name>,
        set_name: &Name,
        source_name: &Name,
    ) -> Result<(), LoadError> {
        let mut glyph_data = crate::util::extract_glyph_data(font, glyphs);

        // Check if some glyphs are already in other sets so we can route them
        // there. Fresh glyphs without an entry can then go into `set_name`.
        let mut glyphs_leftovers = glyphs.clone();
        let mut set_to_glyphs: HashMap<Name, HashSet<Name>> = HashMap::new();
        for (set_name, set) in &self.sets {
            let coverage = set.glyph_coverage();
            let intersection: HashSet<Name> = coverage.intersection(glyphs).cloned().collect();
            if intersection.is_empty() {
                continue;
            }
            glyphs_leftovers.retain(|n| !intersection.contains(n));
            set_to_glyphs.insert(set_name.clone(), intersection);
        }
        if !glyphs_leftovers.is_empty() {
            set_to_glyphs.insert(set_name.clone(), glyphs_leftovers);
        }

        for (set_name, glyph_names) in set_to_glyphs {
            let set = self.sets.entry(set_name.clone()).or_default();
            for name in &glyph_names {
                if let Some((key, value)) = glyph_data.remove_entry(name) {
                    set.glyph_data.insert(key, value);
                }
            }

            let source = set.sources.entry(source_name.clone()).or_insert_with(|| {
                Source::new_with_default_layer_name(font.default_layer().name().clone())
            });
            assert_eq!(source.layers.len(), 1);
            assert!(source.layers.values().next().unwrap().default);

            for layer in font.iter_layers() {
                let our_layer = Layer::from_ufo_layer(layer, &glyph_names);
                if our_layer.glyphs.is_empty() {
                    continue;
                }

                let target_layer = if layer == font.default_layer() {
                    source.get_default_layer_mut()
                } else {
                    source.get_or_create_layer(layer.name().clone())
                };

                target_layer.glyphs.extend(our_layer.glyphs);
                target_layer.color_marks.extend(our_layer.color_marks);
            }
        }

        // TODO: Import glyphs used as components by glyphs on the import list
        // automatically (recursively follow the graph).

        // TODO: Check incoming composites with components outside the import
        // set name: are they different? If so, warn the user. E.g. you import
        // A-cy into Cyrl and the underlying A is different from the A in the
        // import font. Again track diffs recursively in nested composites.

        Ok(())
    }

    pub fn export(
        &self,
        set_names: &HashSet<Name>,
        glyph_names: &HashSet<Name>,
        source_names: &HashSet<Name>,
    ) -> Result<BTreeMap<Name, norad::Font>, ExportError> {
        let mut ufos: BTreeMap<Name, norad::Font> = BTreeMap::new();

        for (_, set) in self
            .sets
            .iter()
            .filter(|(name, _)| set_names.contains(*name))
        {
            for (source_name, source) in set
                .sources
                .iter()
                .filter(|(name, _)| source_names.contains(*name))
            {
                let ufo = ufos.entry(source_name.clone()).or_default();
                for (layer_name, layer) in &source.layers {
                    let layer_glyphs: Vec<_> = layer
                        .glyphs
                        .values()
                        .filter(|g| glyph_names.contains(&*g.name))
                        .collect();
                    if layer_glyphs.is_empty() {
                        continue;
                    }
                    if layer.default {
                        {
                            let ufo_layer = ufo.layers.default_layer_mut();
                            for glyph in layer_glyphs {
                                ufo_layer.insert_glyph(glyph.clone());
                            }
                        }
                        ufo.layers
                            .rename_layer(
                                &ufo.layers.default_layer().name().clone(),
                                layer_name,
                                false,
                            )
                            .unwrap();
                    } else {
                        match ufo.layers.get_mut(layer_name) {
                            Some(ufo_layer) => {
                                for glyph in layer_glyphs {
                                    ufo_layer.insert_glyph(glyph.clone());
                                }
                            }
                            None => {
                                let ufo_layer = ufo
                                    .layers
                                    .new_layer(layer_name)
                                    .expect("can't make new layer");
                                for glyph in layer_glyphs {
                                    ufo_layer.insert_glyph(glyph.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(ufos)
    }
}

impl Set {
    fn from_path(path: &Path) -> Result<Self, LoadError> {
        let glyph_data = metadata::load_glyph_data(&path.join("glyph_data.csv"));

        let mut sources = BTreeMap::new();
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

    pub fn save(&self, set_name: &Name, root_path: &Path) {
        let set_path = root_path.join(format!("set.{set_name}"));
        std::fs::create_dir(&set_path).expect("can't create set dir");

        metadata::write_glyph_data(&self.glyph_data, &set_path.join("glyph_data.csv"));

        for (source_name, source) in &self.sources {
            source.save(source_name, &set_path)
        }
    }

    pub fn glyph_coverage(&self) -> HashSet<Name> {
        let mut glyphs = HashSet::new();
        glyphs.extend(self.glyph_data.keys().cloned());
        for source in self.sources.values() {
            for layer in source.layers.values() {
                glyphs.extend(layer.glyphs.keys().cloned());
            }
        }
        glyphs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty() {
        let tempdir = tempfile::TempDir::new().unwrap();

        let fontgarden = Fontgarden::new();
        fontgarden.save(tempdir.path());
        let fontgarden2 = Fontgarden::from_path(tempdir.path()).unwrap();

        assert_eq!(fontgarden, fontgarden2);
    }

    #[test]
    fn roundtrip_mutatorsans() {
        let mut fontgarden = Fontgarden::new();

        let ufo_paths = [
            "testdata/MutatorSansLightWide.ufo",
            "testdata/MutatorSansLightCondensed.ufo",
        ];

        let latin_set: HashSet<Name> = ["A", "Aacute", "S"]
            .iter()
            .map(|n| Name::new(n).unwrap())
            .collect();

        let punctuation_set: HashSet<Name> = ["quotedblbase", "quotedblleft", "comma"]
            .iter()
            .map(|n| Name::new(n).unwrap())
            .collect();

        let arrow_set: HashSet<Name> = ["arrowleft"]
            .iter()
            .map(|n| Name::new(n).unwrap())
            .collect();

        let default_set: HashSet<Name> = ["acute"].iter().map(|n| Name::new(n).unwrap()).collect();

        for ufo_path in ufo_paths {
            let font = norad::Font::load(ufo_path).unwrap();
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap())
                .unwrap();

            fontgarden
                .import(
                    &font,
                    &latin_set,
                    &Name::new("Latin").unwrap(),
                    &source_name,
                )
                .unwrap();

            fontgarden
                .import(
                    &font,
                    &arrow_set,
                    &Name::new("Arrows").unwrap(),
                    &source_name,
                )
                .unwrap();

            fontgarden
                .import(
                    &font,
                    &punctuation_set,
                    &Name::new("Punctuation").unwrap(),
                    &source_name,
                )
                .unwrap();

            fontgarden
                .import(
                    &font,
                    &default_set,
                    &Name::new("default").unwrap(),
                    &source_name,
                )
                .unwrap();
        }

        // let tempdir = tempfile::tempdir().unwrap();
        let tempdir = std::env::temp_dir().join("testms.fontgarden");
        fontgarden.save(&tempdir);
        let fontgarden2 = Fontgarden::from_path(&tempdir).unwrap();

        for set in fontgarden.sets.values() {
            for source in set.sources.values() {
                for (layer_name, layer) in &source.layers {
                    if layer_name.as_ref() == "foreground" {
                        assert!(layer.default)
                    } else {
                        assert!(!layer.default)
                    }
                }
            }
        }

        use pretty_assertions::assert_eq;
        assert_eq!(fontgarden, fontgarden2);
    }
}
