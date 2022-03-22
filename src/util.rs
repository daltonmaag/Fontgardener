use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use norad::Name;

use crate::structs::*;

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

pub(crate) fn follow_composites(
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

pub(crate) fn follow_components(
    fontgarden: &Fontgarden,
    name: Name,
    reverse_coverage: &HashMap<Name, Name>,
) -> HashSet<Name> {
    let mut discovered_glyphs = HashSet::new();
    let mut stack = Vec::new();

    fn collect_glyph_component_names_from_set(
        fontgarden: &Fontgarden,
        set_name: Name,
        name: Name,
    ) -> HashSet<Name> {
        let mut component_names = HashSet::new();
        for source in fontgarden.sets[&set_name].sources.values() {
            for layer in source.layers.values() {
                if let Some(glyph) = layer.glyphs.get(&name) {
                    component_names.extend(glyph.components.iter().map(|c| c.base.clone()));
                }
            }
        }
        component_names
    }

    let set = &fontgarden.sets[&reverse_coverage[&name]];
    for source in set.sources.values() {
        for layer in source.layers.values() {
            if let Some(glyph) = layer.glyphs.get(&name) {
                for component in &glyph.components {
                    stack.push(component.base.clone());
                    // TODO: guard against loops
                    while let Some(component_name) = stack.pop() {
                        // TODO: guard against non-existent glyphs
                        let component_names = collect_glyph_component_names_from_set(
                            fontgarden,
                            reverse_coverage[&component_name].clone(),
                            glyph.name.clone(),
                        );

                        discovered_glyphs.insert(component_name.clone());
                        stack.extend(
                            component_names
                                .into_iter()
                                .filter(|v| !discovered_glyphs.contains(v)),
                        );
                    }
                }
                assert!(stack.is_empty());
            }
        }
    }

    discovered_glyphs
}
