use std::{
    collections::{HashMap, HashSet},
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
    pub opentype_category: Option<String>,
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
}
