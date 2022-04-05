use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use norad::Name;

use crate::structs::GlyphRecord;

pub(crate) fn extract_glyph_data(
    font: &norad::Font,
    glyphs: &HashSet<Name>,
) -> BTreeMap<Name, GlyphRecord> {
    let mut glyph_data: BTreeMap<Name, GlyphRecord> = BTreeMap::new();

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

pub(crate) fn glyphset_follow_composites(
    import_glyphs: &HashSet<Name>,
    components_in_glyph: impl Fn(Name) -> Vec<Name>,
) -> HashSet<Name> {
    let mut discovered_glyphs = import_glyphs.clone();

    let mut stack = Vec::new();
    for name in import_glyphs.iter() {
        // TODO: guard against loops and hanging components.
        for component in components_in_glyph(name.clone()) {
            stack.push(component);
            while let Some(component) = stack.pop() {
                let new_components = components_in_glyph(component);
                discovered_glyphs.extend(new_components.iter().cloned());
                stack.extend(new_components.into_iter().rev())
            }
        }
        assert!(stack.is_empty());
    }

    discovered_glyphs
}

// TODO: Unify follow_components for fontgarden and norad::font
pub(crate) fn ufo_follow_composites(
    font: &norad::Font,
    import_glyphs: &HashSet<Name>,
) -> HashSet<Name> {
    let mut discovered_glyphs = import_glyphs.clone();
    let mut stack = Vec::new();
    for name in import_glyphs.iter() {
        let glyph = font
            .get_glyph(name)
            .unwrap_or_else(|| panic!("glyph {name} not in font"));

        for component in &glyph.components {
            stack.push(component);
            // TODO: guard against loops
            while let Some(component) = stack.pop() {
                let new_glyph = font
                    .get_glyph(&component.base)
                    .unwrap_or_else(|| panic!("glyph {} not in font", &component.base));
                discovered_glyphs.insert(new_glyph.name.clone());
                for new_component in new_glyph.components.iter().rev() {
                    stack.push(new_component);
                }
            }
        }
        assert!(stack.is_empty());
    }
    discovered_glyphs
}
