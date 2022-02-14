use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use norad::Name;

mod lib;

const NOTO_TEMP: &str = r"C:\Users\nikolaus.waxweiler\AppData\Local\Dev\nototest";

fn main() {
    let tmp_path = Path::new(NOTO_TEMP);

    let latin_set_name = Name::new("Latin").unwrap();
    let latin_glyphs = HashSet::from(["A", "B", "Adieresis", "Omega"]);

    let ufo_lt = norad::Font::load(tmp_path.join("NotoSans-Light.ufo")).unwrap();
    let source_light_name = Name::new("Light").unwrap();
    let mut source_light = lib::Source::default();
    for layer in ufo_lt.iter_layers() {
        let our_layer = lib::Layer::from_ufo_layer(layer, &latin_glyphs);
        source_light.layers.insert(layer.name().clone(), our_layer);
    }

    let ufo_bd = norad::Font::load(tmp_path.join("NotoSans-Bold.ufo")).unwrap();
    let source_bold_name = Name::new("Bold").unwrap();
    let mut source_bold = lib::Source::default();
    for layer in ufo_bd.iter_layers() {
        let our_layer = lib::Layer::from_ufo_layer(layer, &latin_glyphs);
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

    let mut latin_glyph_data: HashMap<Name, lib::GlyphRecord> = HashMap::new();
    for name in latin_glyphs {
        let mut record = lib::GlyphRecord {
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
    let latin_set = lib::Set {
        glyph_data: latin_glyph_data,
        sources,
    };

    let mut sets = HashMap::new();
    sets.insert(latin_set_name, latin_set);
    let fontgarden = lib::Fontgarden { sets };

    println!("{:#?}", &fontgarden);

    fontgarden.save(&tmp_path.join("test.fontgarden"));
}
