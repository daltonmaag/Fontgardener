use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    path::Path,
    str::FromStr,
};

use norad::{Color, Name};
use serde::{Deserialize, Serialize};

use crate::errors::{
    ExportError, LoadError, LoadGlyphDataError, LoadLayerError, LoadSetError, LoadSourceError,
    SaveError, SaveLayerError, SaveSetError, SaveSourceError,
};

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

#[derive(Debug, PartialEq)]
pub struct Source {
    // TODO: UFO layers are ordered, export from here will always sort order.
    // Relevant other than in testing?
    pub layers: BTreeMap<Name, Layer>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Layer {
    pub glyphs: BTreeMap<Name, norad::Glyph>,
    pub color_marks: BTreeMap<Name, norad::Color>,
    pub default: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LayerInfo {
    pub name: Name,
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

impl Fontgarden {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_path(path: &Path) -> Result<Self, LoadError> {
        let mut fontgarden = Self::new();
        let mut seen_glyph_names: HashSet<Name> = HashSet::new();

        if !path.is_dir() {
            return Err(LoadError::NotAFontgarden);
        }

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                // TODO: Figure out when this call is None and if we should deal
                // with it.
                if let Some(file_name) = path.file_name() {
                    if let Some(set_name) = file_name.to_string_lossy().strip_prefix("set.") {
                        let set_name = Name::new(set_name)
                            .map_err(|e| LoadError::NamingError(set_name.into(), e))?;

                        let set = Set::from_path(&path)
                            .map_err(|e| LoadError::LoadSet(set_name.clone(), e))?;

                        let coverage = set.glyph_coverage();
                        let overlapping_coverage: HashSet<Name> =
                            seen_glyph_names.intersection(&coverage).cloned().collect();
                        if !overlapping_coverage.is_empty() {
                            return Err(LoadError::DuplicateGlyphs(set_name, overlapping_coverage));
                        }
                        seen_glyph_names.extend(coverage);

                        fontgarden.sets.insert(set_name.clone(), set);
                    }
                }
            }
        }

        Ok(fontgarden)
    }

    pub fn save(&self, path: &Path) -> Result<(), SaveError> {
        if path.exists() {
            std::fs::remove_dir_all(path).map_err(SaveError::Cleanup)?;
        }
        std::fs::create_dir(path).map_err(SaveError::CreateDir)?;

        for (set_name, set) in &self.sets {
            set.save(set_name, path)
                .map_err(|e| SaveError::SaveSet(set_name.clone(), e))?;
        }

        Ok(())
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
            assert_eq!(source.layers.values().filter(|l| l.default).count(), 1);

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

    fn assemble_sources(&self, source_names: &HashSet<Name>) -> HashMap<Name, Source> {
        let mut assembled_sources: HashMap<Name, Source> = HashMap::new();

        for set in self.sets.values() {
            for (source_name, source) in set
                .sources
                .iter()
                .filter(|(name, _)| source_names.contains(*name))
            {
                let assembled_source = assembled_sources.entry(source_name.clone()).or_default();
                for (layer_name, layer) in source.layers.iter() {
                    let assembled_layer = assembled_source
                        .layers
                        .entry(layer_name.clone())
                        .or_default();
                    assembled_layer.glyphs.extend(layer.glyphs.clone());
                    assembled_layer
                        .color_marks
                        .extend(layer.color_marks.clone());
                    // TODO: guard against different default layers having different names?
                    assembled_layer.default = layer.default;
                }
            }
        }

        assembled_sources
    }

    pub fn export(
        &self,
        glyph_names: &HashSet<Name>,
        source_names: &HashSet<Name>,
    ) -> Result<BTreeMap<Name, norad::Font>, ExportError> {
        let mut ufos: BTreeMap<Name, norad::Font> = BTreeMap::new();

        // First, make a copy of self and prune sources and glyphs not in the
        // export sets.
        let mut sources = self.assemble_sources(source_names);
        for source in sources.values_mut() {
            for layer in source.layers.values_mut() {
                let glyph_names = crate::util::glyphset_follow_composites(glyph_names, |n| {
                    layer
                        .glyphs
                        .get(&n)
                        .map(|g| g.components.iter().map(|c| c.base.clone()).collect())
                        .unwrap_or_default()
                });
                layer.glyphs.retain(|name, _| glyph_names.contains(name));
                layer
                    .color_marks
                    .retain(|name, _| glyph_names.contains(name));
            }
        }

        // Then, transform the pruned tree into UFO structures.
        for (source_name, source) in sources {
            let ufo = ufos.entry(source_name.clone()).or_default();
            for (layer_name, layer) in source.layers {
                if layer.glyphs.is_empty() {
                    continue;
                }

                if layer.default {
                    {
                        let ufo_layer = ufo.layers.default_layer_mut();
                        layer.into_ufo_layer(ufo_layer);
                    }
                    // TODO: be smarter about naming default layers?
                    if layer_name != *ufo.layers.default_layer_mut().name() {
                        ufo.layers
                            .rename_layer(
                                &ufo.layers.default_layer().name().clone(),
                                &layer_name,
                                false,
                            )
                            .unwrap();
                    }
                } else {
                    let ufo_layer = match ufo.layers.get_mut(&layer_name) {
                        Some(ufo_layer) => ufo_layer,
                        None => ufo
                            .layers
                            .new_layer(&layer_name)
                            .expect("can't make new layer"),
                    };
                    layer.into_ufo_layer(ufo_layer);
                }
            }
        }

        Ok(ufos)
    }
}

impl Set {
    fn from_path(path: &Path) -> Result<Self, LoadSetError> {
        let glyph_data = Self::load_glyph_data(&path.join("glyph_data.csv"))
            .map_err(LoadSetError::LoadGlyphData)?;

        let mut sources = BTreeMap::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                // TODO: Figure out when this call is None and if we should deal
                // with it.
                if let Some(file_name) = path.file_name() {
                    if let Some(source_name) = file_name.to_string_lossy().strip_prefix("source.") {
                        let source_name = Name::new(source_name)
                            .map_err(|e| LoadSetError::NamingError(source_name.into(), e))?;
                        let source = Source::from_path(&path)
                            .map_err(|e| LoadSetError::LoadSource(source_name.clone(), e))?;
                        sources.insert(source_name, source);
                    }
                }
            }
        }

        Ok(Set {
            glyph_data,
            sources,
        })
    }

    pub fn save(&self, set_name: &Name, root_path: &Path) -> Result<(), SaveSetError> {
        let set_path = root_path.join(format!("set.{set_name}"));
        std::fs::create_dir(&set_path).map_err(SaveSetError::CreateDir)?;

        Self::write_glyph_data(&self.glyph_data, &set_path.join("glyph_data.csv"))
            .map_err(SaveSetError::WriteGlyphData)?;

        for (source_name, source) in &self.sources {
            source
                .save(source_name, &set_path)
                .map_err(|e| SaveSetError::SaveSource(source_name.clone(), e))?;
        }

        Ok(())
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

    fn load_glyph_data(path: &Path) -> Result<BTreeMap<Name, GlyphRecord>, LoadGlyphDataError> {
        let mut glyph_data = BTreeMap::new();
        let mut reader = csv::Reader::from_path(path).map_err(LoadGlyphDataError::Csv)?;

        type Record = (String, Option<String>, Option<String>, Option<String>, bool);
        for result in reader.deserialize() {
            let record: Record = result.map_err(LoadGlyphDataError::Csv)?;

            let glyph_name = Name::new(&record.0)
                .map_err(|e| LoadGlyphDataError::InvalidGlyphName(record.0, e))?;
            let codepoints = match &record.2 {
                Some(codepoints_string) => {
                    Self::parse_codepoints(codepoints_string).map_err(|e| {
                        LoadGlyphDataError::InvalidCodepoint(
                            glyph_name.clone(),
                            codepoints_string.clone(),
                            e,
                        )
                    })?
                }
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

    fn write_glyph_data(
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
}

impl Default for Source {
    fn default() -> Self {
        let layer = Layer {
            default: true,
            ..Default::default()
        };
        Self {
            layers: BTreeMap::from([(Name::new("public.default").unwrap(), layer)]),
        }
    }
}

impl Source {
    pub(crate) fn from_path(path: &Path) -> Result<Self, LoadSourceError> {
        let mut layers = BTreeMap::new();
        let mut found_default = false;

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                let metadata = entry.metadata()?;
                if metadata.is_dir()
                    && (file_name == "glyphs" || file_name.to_string_lossy().starts_with("glyphs."))
                {
                    let (layer, layerinfo) = Layer::from_path(&path)
                        .map_err(|e| LoadSourceError::LoadLayer(path.clone(), e))?;

                    // All non-default layer names start with a dot after "glyphs".
                    // Hope that we don't bump into filesystem case-sensitivity
                    // issues.
                    if file_name == "glyphs" {
                        found_default = true;
                    }
                    layers.insert(layerinfo.name, layer);
                }
            }
        }

        if !found_default {
            return Err(LoadSourceError::NoDefaultLayer);
        }

        Ok(Source { layers })
    }

    pub fn get_default_layer_mut(&mut self) -> &mut Layer {
        self.layers
            .values_mut()
            .find(|layer| layer.default)
            .unwrap()
    }

    pub fn get_or_create_layer(&mut self, name: Name) -> &mut Layer {
        self.layers.entry(name).or_default()
    }

    pub fn new_with_default_layer_name(name: Name) -> Self {
        let layer = Layer {
            default: true,
            ..Default::default()
        };
        Self {
            layers: BTreeMap::from([(name, layer)]),
        }
    }

    pub(crate) fn save(&self, source_name: &str, set_path: &Path) -> Result<(), SaveSourceError> {
        let source_path = set_path.join(format!("source.{source_name}"));
        std::fs::create_dir(&source_path).map_err(SaveSourceError::CreateDir)?;

        let mut existing_layer_names = HashSet::new();
        for (layer_name, layer) in &self.layers {
            layer
                .save(layer_name, &source_path, &mut existing_layer_names)
                .map_err(|e| SaveSourceError::SaveLayer(layer_name.clone(), e))?;
        }

        Ok(())
    }
}

impl Layer {
    pub(crate) fn from_path(path: &Path) -> Result<(Self, LayerInfo), LoadLayerError> {
        let mut glyphs = BTreeMap::new();
        let color_marks = Self::load_color_marks(&path.join("color_marks.csv"))
            .map_err(LoadLayerError::LoadColorMarks)?;
        let layerinfo: LayerInfo = plist::from_file(path.join("layerinfo.plist"))
            .map_err(LoadLayerError::LoadLayerInfo)?;

        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_file() && path.extension().map_or(false, |n| n == "glif") {
                let glif = norad::Glyph::load(&path)
                    .map_err(|e| LoadLayerError::LoadGlyph(path.clone(), e))?;
                glyphs.insert(glif.name.clone(), glif);
            }
        }

        Ok((
            Layer {
                glyphs,
                color_marks,
                default: path.file_name() == Some(OsStr::new("glyphs")),
            },
            layerinfo,
        ))
    }

    pub(crate) fn from_ufo_layer(layer: &norad::Layer, glyph_names: &HashSet<Name>) -> Self {
        let mut glyphs = BTreeMap::new();
        let mut color_marks = BTreeMap::new();

        for glyph in layer
            .iter()
            .filter(|g| glyph_names.contains(g.name.as_str()))
        {
            let mut our_glyph = glyph.clone();
            if let Some(color_string) = our_glyph.lib.remove("public.markColor") {
                // FIXME: We roundtrip color here so that we round up front to
                // make roundtrip equality testing easier.
                let our_color = Color::from_str(color_string.as_string().unwrap()).unwrap();
                let our_color = Color::from_str(&our_color.to_rgba_string()).unwrap();
                color_marks.insert(glyph.name.clone(), our_color);
            }
            // TODO: split out the codepoints.
            glyphs.insert(glyph.name.clone(), our_glyph);
        }

        Self {
            glyphs,
            color_marks,
            default: false,
        }
    }

    pub(crate) fn into_ufo_layer(self, ufo_layer: &mut norad::Layer) {
        for (name, mut glyph) in self.glyphs {
            if let Some(c) = self.color_marks.get(&name) {
                glyph
                    .lib
                    .insert("public.markColor".into(), c.to_rgba_string().into());
            }
            ufo_layer.insert_glyph(glyph);
        }
    }

    fn load_color_marks(path: &Path) -> Result<BTreeMap<Name, Color>, csv::Error> {
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

    pub(crate) fn save(
        &self,
        layer_name: &Name,
        source_path: &Path,
        existing_layer_names: &mut HashSet<String>,
    ) -> Result<(), SaveLayerError> {
        if self.glyphs.is_empty() {
            return Ok(());
        }

        let layer_path = if self.default {
            source_path.join("glyphs")
        } else {
            let path = source_path.join(norad::util::default_file_name_for_layer_name(
                layer_name,
                existing_layer_names,
            ));
            existing_layer_names.insert(path.to_string_lossy().to_string());
            path
        };
        std::fs::create_dir(&layer_path).map_err(SaveLayerError::CreateDir)?;

        plist::to_file_xml(
            layer_path.join("layerinfo.plist"),
            &LayerInfo {
                name: layer_name.clone(),
            },
        )
        .map_err(SaveLayerError::WriteLayerInfo)?;

        let mut existing_glyph_names = HashSet::new();
        for (glyph_name, glyph) in &self.glyphs {
            let filename =
                norad::util::default_file_name_for_glyph_name(glyph_name, &existing_glyph_names);
            let glyph_path = layer_path.join(&filename);
            glyph
                .save(&glyph_path)
                .map_err(|e| SaveLayerError::SaveGlyph(glyph_name.clone(), e))?;
            existing_glyph_names.insert(filename.to_string_lossy().to_string());
        }

        Self::write_color_marks(&layer_path.join("color_marks.csv"), &self.color_marks)
            .map_err(SaveLayerError::WriteColorMarks)?;

        Ok(())
    }

    fn write_color_marks(
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
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use norad::Color;

    use super::*;

    #[test]
    fn load_empty() {
        let tempdir = tempfile::TempDir::new().unwrap();

        let fontgarden = Fontgarden::new();
        fontgarden.save(tempdir.path()).unwrap();
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

        let tempdir = tempfile::tempdir().unwrap();
        fontgarden.save(tempdir.path()).unwrap();
        let fontgarden2 = Fontgarden::from_path(tempdir.path()).unwrap();

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

    #[test]
    fn roundtrip_mutatorsans_export_import() {
        let mut fontgarden = Fontgarden::new();

        let mut ufo_lightwide = norad::Font::load("testdata/MutatorSansLightWide.ufo").unwrap();
        let mut ufo_lightcond =
            norad::Font::load("testdata/MutatorSansLightCondensed.ufo").unwrap();

        // TODO: find workaround for equality testing color accuracy.
        for ufo in [&mut ufo_lightwide, &mut ufo_lightcond] {
            let layer_names: Vec<_> = ufo.layers.iter().map(|l| l.name()).cloned().collect();
            for layer_name in layer_names {
                let layer = ufo.layers.get_mut(&layer_name).unwrap();
                for glyph in layer.iter_mut() {
                    if let Some(color_string) = glyph.lib.remove("public.markColor") {
                        // FIXME: We roundtrip color here so that we round up front to
                        // make roundtrip equality testing easier.
                        let our_color = Color::from_str(color_string.as_string().unwrap()).unwrap();
                        let our_color = Color::from_str(&our_color.to_rgba_string()).unwrap();
                        glyph
                            .lib
                            .insert("public.markColor".into(), our_color.to_rgba_string().into());
                    }
                }
            }
        }

        let name_latin = Name::new("Latin").unwrap();
        let name_default = Name::new("default").unwrap();

        let latin_set: HashSet<Name> = ["A", "Aacute", "S"]
            .iter()
            .map(|n| Name::new(n).unwrap())
            .collect();

        let mut glyph_names = HashSet::new();
        let mut source_names = HashSet::new();
        for font in [&ufo_lightwide, &ufo_lightcond] {
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap())
                .unwrap();
            glyph_names.extend(font.iter_names());
            source_names.insert(source_name.clone());

            fontgarden
                .import(font, &latin_set, &name_latin, &source_name)
                .unwrap();

            fontgarden
                .import(
                    font,
                    &HashSet::from_iter(font.iter_names())
                        .difference(&latin_set)
                        .cloned()
                        .collect(),
                    &name_default,
                    &source_name,
                )
                .unwrap();
        }

        let roundtripped_ufos = fontgarden.export(&glyph_names, &source_names).unwrap();

        assert_font_eq(&ufo_lightwide, &roundtripped_ufos["LightWide"]);
        assert_font_eq(&ufo_lightcond, &roundtripped_ufos["LightCondensed"]);
    }

    fn assert_font_eq(reference: &norad::Font, other: &norad::Font) {
        // TODO: compare more than glyphs.
        for reference_layer in reference.layers.iter() {
            let reference_glyphs: Vec<_> = reference_layer.iter().collect();
            let other_layer = other.layers.get(reference_layer.name()).unwrap();
            let other_glyphs: Vec<_> = other_layer.iter().collect();
            assert_eq!(reference_glyphs, other_glyphs);
        }
    }

    #[test]
    fn update_sets() {
        let mut fontgarden = Fontgarden::new();

        let mut ufo_lightwide = norad::Font::load("testdata/MutatorSansLightWide.ufo").unwrap();
        let mut ufo_lightcond =
            norad::Font::load("testdata/MutatorSansLightCondensed.ufo").unwrap();

        // TODO: compare glyphs differently so color marks don't matter.
        for ufo in [&mut ufo_lightwide, &mut ufo_lightcond] {
            let layer_names: Vec<_> = ufo.layers.iter().map(|l| l.name()).cloned().collect();
            for layer_name in layer_names {
                let layer = ufo.layers.get_mut(&layer_name).unwrap();
                for glyph in layer.iter_mut() {
                    glyph.lib.remove("public.markColor");
                }
            }
        }

        let name_latin = Name::new("Latin").unwrap();
        let name_default = Name::new("default").unwrap();
        let name_a = Name::new("A").unwrap();
        let name_arrowleft = Name::new("arrowleft").unwrap();

        let latin_set = HashSet::from([name_a]);
        let default_set = HashSet::from([name_arrowleft]);

        for font in [&ufo_lightwide, &ufo_lightcond] {
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap())
                .unwrap();

            fontgarden
                .import(font, &latin_set, &name_latin, &source_name)
                .unwrap();
            fontgarden
                .import(font, &default_set, &name_default, &source_name)
                .unwrap();
        }

        assert_eq!(
            &fontgarden.sets["Latin"].sources["LightWide"].layers["foreground"].glyphs["A"],
            ufo_lightwide.get_glyph("A").unwrap()
        );
        assert_eq!(
            &fontgarden.sets["default"].sources["LightCondensed"].layers["foreground"].glyphs
                ["arrowleft"],
            ufo_lightcond.get_glyph("arrowleft").unwrap()
        );

        ufo_lightwide
            .get_glyph_mut("A")
            .unwrap()
            .lib
            .insert("aaaa".into(), 1.into());
        ufo_lightcond
            .get_glyph_mut("arrowleft")
            .unwrap()
            .lib
            .insert("bbbb".into(), 1.into());

        for font in [&ufo_lightwide, &ufo_lightcond] {
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap())
                .unwrap();

            fontgarden
                .import(
                    font,
                    &latin_set.union(&default_set).cloned().collect(),
                    &name_latin,
                    &source_name,
                )
                .unwrap();
        }

        assert_eq!(
            &fontgarden.sets["Latin"].sources["LightWide"].layers["foreground"].glyphs["A"],
            ufo_lightwide.get_glyph("A").unwrap()
        );
        assert_eq!(
            &fontgarden.sets["default"].sources["LightCondensed"].layers["foreground"].glyphs
                ["arrowleft"],
            ufo_lightcond.get_glyph("arrowleft").unwrap()
        );
    }

    #[test]
    fn roundtrip_mutatorsans_follow_components() {
        let mut fontgarden = Fontgarden::new();

        let ufo_paths = [
            "testdata/MutatorSansLightWide.ufo",
            "testdata/MutatorSansLightCondensed.ufo",
        ];

        let set_name = Name::new("Latin").unwrap();
        let glyphs: HashSet<Name> = HashSet::from([Name::new("Aacute").unwrap()]);
        let glyphs_expected: HashSet<Name> =
            HashSet::from(["A", "Aacute", "acute"].map(|n| Name::new(n).unwrap()));

        for ufo_path in ufo_paths {
            let font = norad::Font::load(ufo_path).unwrap();
            let source_name = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap())
                .unwrap();

            let glyphs = crate::util::ufo_follow_composites(&font, &glyphs);
            fontgarden
                .import(&font, &glyphs, &set_name, &source_name)
                .unwrap();
        }

        for (set_name, set) in fontgarden.sets.iter() {
            for (source_name, source) in set.sources.iter() {
                for (layer_name, layer) in source.layers.iter() {
                    assert!(
                        // Some layers may contain the "A" but not the "Aacute".
                        HashSet::from_iter(layer.glyphs.keys().cloned())
                            .is_subset(&glyphs_expected),
                        "Set {set_name}, source {source_name}, layer {layer_name}"
                    );
                }
            }
        }

        let source_names =
            HashSet::from(["LightWide", "LightCondensed"].map(|n| Name::new(n).unwrap()));
        let exports = fontgarden.export(&glyphs, &source_names).unwrap();

        for (font_name, font) in exports.iter() {
            for layer in font.layers.iter() {
                assert!(
                    // Some layers may contain the "A" but not the "Aacute".
                    HashSet::from_iter(layer.iter().map(|g| g.name.clone()))
                        .is_subset(&glyphs_expected),
                    "Font {font_name}, layer {}",
                    layer.name()
                );
            }
        }
    }
}