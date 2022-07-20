use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use norad::Name;

use crate::structs::GlyphRecord;

// TODO: Refactor to return errors
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
            codepoints: font
                .get_glyph(name)
                .map(|g| g.codepoints.clone())
                .unwrap_or_default(),
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

// TODO: Refactor to return errors
pub(crate) fn load_glyph_list(path: &Path) -> Result<HashSet<Name>, std::io::Error> {
    let names: HashSet<Name> = std::fs::read_to_string(path)?
        .lines()
        .map(|s| s.trim()) // Remove whitespace for line
        .filter(|s| !s.is_empty()) // Drop now empty lines
        .map(|v| Name::new(v).unwrap())
        .collect();
    Ok(names)
}

/// Resolves a glyph list to also include all glyphs referenced as a component.
///
/// NOTE: Silently ignores hanging components.
///
/// TODO: Guard against loops. Or do we already?
/// TODO: Be smarter by skipping glyphs already discovered?
pub(crate) fn glyphset_follow_composites(
    import_glyphs: &HashSet<Name>,
    components_in_glyph: impl Fn(Name) -> Vec<Name>,
) -> HashSet<Name> {
    let mut discovered_glyphs = import_glyphs.clone();

    let mut stack = Vec::new();
    for name in import_glyphs.iter() {
        stack.extend(components_in_glyph(name.clone()));
        while let Some(component) = stack.pop() {
            // TODO: are we properly preventing looping or repeat checking?
            if discovered_glyphs.insert(component.clone()) {
                let new_components = components_in_glyph(component.clone());
                stack.extend(new_components.into_iter().rev())
            }
        }
        assert!(stack.is_empty());
    }

    discovered_glyphs
}

pub(crate) fn guess_source_name(font: &norad::Font) -> Option<Name> {
    match font.font_info.style_name.as_ref() {
        Some(string) => match Name::new(string) {
            Ok(name) => Some(name),
            Err(_) => None,
        },
        None => None,
    }
}

/// Given a glyph `name`, return an appropriate file name.
/// 
/// NOTE: Copied from norad 0.7
pub fn default_file_name_for_glyph_name(name: &Name, existing: &HashSet<String>) -> PathBuf {
    user_name_to_file_name(name, "", ".glif", existing)
}

/// Given a layer `name`, return an appropriate file name.
///
/// NOTE: Copied from norad 0.7
pub fn default_file_name_for_layer_name(name: &Name, existing: &HashSet<String>) -> PathBuf {
    user_name_to_file_name(name, "glyphs.", "", existing)
}

/// Given a `name`, return an appropriate file name.
///
/// Expects `existing` to be a set of paths (potentially lossily) converted to
/// _lowercased [`String`]s_. The file names are going to end up in a UTF-8
/// encoded Apple property list XML file, so file names will be treated as
/// Unicode strings.
///
/// # Panics
///
/// Panics if a case-insensitive file name clash was detected and no unique
/// value could be created after 99 numbering attempts.
/// 
/// NOTE: Copied from norad 0.7
fn user_name_to_file_name(
    name: &Name,
    prefix: &str,
    suffix: &str,
    existing: &HashSet<String>,
) -> PathBuf {
    let mut result = String::with_capacity(prefix.len() + name.len() + suffix.len());

    // Filter illegal characters from name.
    static SPECIAL_ILLEGAL: &[char] = &[
        ':', '?', '"', '(', ')', '[', ']', '*', '/', '\\', '+', '<', '>', '|',
    ];

    // Assert that the prefix and suffix are safe, as they should be controlled
    // by norad.
    debug_assert!(
        !prefix.chars().any(|c| SPECIAL_ILLEGAL.contains(&c)),
        "prefix must not contain illegal chars"
    );
    debug_assert!(
        suffix.is_empty() || suffix.starts_with('.'),
        "suffix must be empty or start with a period"
    );
    debug_assert!(
        !suffix.chars().any(|c| SPECIAL_ILLEGAL.contains(&c)),
        "suffix must not contain illegal chars"
    );
    debug_assert!(
        !suffix.ends_with(['.', ' ']),
        "suffix must not end in period or space"
    );

    result.push_str(prefix);
    for c in name.chars() {
        match c {
            // Replace an initial period with an underscore if there is no
            // prefix to be added, e.g. for the bare glyph name ".notdef".
            '.' if result.is_empty() => result.push('_'),
            // Replace illegal characters with an underscore.
            c if SPECIAL_ILLEGAL.contains(&c) => result.push('_'),
            // Append an underscore to all uppercase characters.
            c if c.is_uppercase() => {
                result.push(c);
                result.push('_');
            }
            // Append the rest unchanged.
            c => result.push(c),
        }
    }

    // Test for reserved names and parts. The relevant part is the prefix + name
    // (or "stem") of the file, so e.g. "com1.glif" would be replaced by
    // "_com1.glif", but "hello.com1.glif", "com10.glif" and "acom1.glif" stay
    // as they are. For algorithmic simplicity, ignore the presence of the
    // suffix and potentially replace more than we strictly need to.
    //
    // List taken from
    // <https://docs.microsoft.com/en-gb/windows/win32/fileio/naming-a-file#naming-conventions>.
    static SPECIAL_RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if let Some(stem) = result.split('.').next() {
        // At this stage, we only need to look for lowercase matches, as every
        // uppercase letter will be followed by an underscore, automatically
        // making the name safe.
        if SPECIAL_RESERVED.contains(&stem) {
            result.insert(0, '_');
        }
    }

    // Clip prefix + name to 255 characters.
    const MAX_LEN: usize = 255;
    if result.len().saturating_add(suffix.len()) > MAX_LEN {
        let mut boundary = MAX_LEN.saturating_sub(suffix.len());
        while !result.is_char_boundary(boundary) {
            boundary -= 1;
        }
        result.truncate(boundary);
    }

    // Replace trailing periods and spaces by underscores unless we have a
    // suffix (which we asserted is safe).
    if suffix.is_empty() && result.ends_with(['.', ' ']) {
        let mut boundary = result.len();
        for (i, c) in result.char_indices().rev() {
            if c != '.' && c != ' ' {
                break;
            }
            boundary = i;
        }
        let underscores = "_".repeat(result.len() - boundary);
        result.replace_range(boundary..result.len(), &underscores);
    }

    result.push_str(suffix);

    // Test for clashes. Use a counter with 2 digits to look for a name not yet
    // taken. The UFO specification recommends using 15 digits and lists a
    // second way should one exhaust them, but it is unlikely to be needed in
    // practice. 1e15 numbers is a ridicuously high number where holding all
    // those glyph names in memory would exhaust it.
    if existing.contains(&result.to_lowercase()) {
        // First, cut off the suffix (plus the space needed for the number
        // counter if necessary).
        const NUMBER_LEN: usize = 2;
        if result
            .len()
            .saturating_sub(suffix.len())
            .saturating_add(NUMBER_LEN)
            > MAX_LEN
        {
            let mut boundary = MAX_LEN
                .saturating_sub(suffix.len())
                .saturating_sub(NUMBER_LEN);
            while !result.is_char_boundary(boundary) {
                boundary -= 1;
            }
            result.truncate(boundary);
        } else {
            // Cutting off the suffix should land on a `char` boundary.
            result.truncate(result.len().saturating_sub(suffix.len()));
        }

        let mut found_unique = false;
        for counter in 1..100u8 {
            result.push_str(&format!("{:0>2}", counter));
            result.push_str(suffix);
            if !existing.contains(&result.to_lowercase()) {
                found_unique = true;
                break;
            }
            result.truncate(result.len().saturating_sub(suffix.len()) - NUMBER_LEN);
        }
        if !found_unique {
            // Note: if this is ever hit, try appending a UUIDv4 before panicing.
            panic!("Could not find a unique file name after 99 tries")
        }
    }

    result.into()
}
