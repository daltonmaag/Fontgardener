use std::collections::BTreeMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::create_dir;
use std::fs::read_dir;
use std::path::Path;
use std::str::FromStr;

use norad::util::default_file_name_for_glyph_name;
use norad::util::default_file_name_for_layer_name;
use norad::Color;
use norad::Name;
use serde::{Deserialize, Serialize};

use super::metadata::{load_color_marks, write_color_marks};
use super::LoadError;

#[derive(Debug, Default, PartialEq)]
pub struct Layer {
    pub glyphs: BTreeMap<Name, norad::Glyph>,
    pub color_marks: BTreeMap<Name, norad::Color>,
    pub default: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerInfo {
    pub name: Name,
}

impl Layer {
    pub(crate) fn from_path(path: &Path) -> Result<(Self, LayerInfo), LoadError> {
        let mut glyphs = BTreeMap::new();
        let color_marks = load_color_marks(&path.join("color_marks.csv"));
        let layerinfo: LayerInfo =
            plist::from_file(path.join("layerinfo.plist")).expect("can't load layerinfo");

        for entry in read_dir(path)? {
            let path = entry?.path();
            if path.is_file() && path.extension().map_or(false, |n| n == "glif") {
                let glif = norad::Glyph::load(&path).expect("can't load glif");
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

    pub(crate) fn save(
        &self,
        layer_name: &Name,
        source_path: &Path,
        existing_layer_names: &mut HashSet<String>,
    ) {
        if self.glyphs.is_empty() {
            return;
        }
        let layer_path = if self.default {
            source_path.join("glyphs")
        } else {
            let path = source_path.join(default_file_name_for_layer_name(
                layer_name,
                existing_layer_names,
            ));
            existing_layer_names.insert(path.to_string_lossy().to_string());
            path
        };
        create_dir(&layer_path).expect("can't create layer dir");

        plist::to_file_xml(
            layer_path.join("layerinfo.plist"),
            &LayerInfo {
                name: layer_name.clone(),
            },
        )
        .expect("can't write layerinfo");

        let mut existing_glyph_names = HashSet::new();
        for (glyph_name, glyph) in &self.glyphs {
            let filename = default_file_name_for_glyph_name(glyph_name, &existing_glyph_names);
            let glyph_path = layer_path.join(&filename);
            glyph.save(&glyph_path).expect("can't write glif file");
            existing_glyph_names.insert(filename.to_string_lossy().to_string());
        }

        write_color_marks(&layer_path.join("color_marks.csv"), &self.color_marks);
    }
}
