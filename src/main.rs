use std::{collections::HashSet, path::PathBuf};

use clap::{Parser, Subcommand};
use norad::Name;

mod structs;

#[derive(Parser)]
#[clap(author, version, about, long_about = None, propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    New {
        /// Fontgarden package path to create.
        #[clap(parse(from_os_str))]
        path: PathBuf,
    },
    Import {
        /// Fontgarden package path to import into.
        #[clap(parse(from_os_str), value_name = "FONTGARDEN_PATH")]
        fontgarden_path: PathBuf,

        /// Text file of glyphs to import, one per line.
        #[clap(parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyph_names_file: PathBuf,

        /// Set to import glyphs into.
        #[clap(long, default_value = "default")]
        set_name: Name,

        /// Source to import glyphs into [default: infer from style name].
        #[clap(long)]
        source_name: Option<Name>,

        /// Unified Font Object (UFO) to import from.
        #[clap(parse(from_os_str))]
        font: PathBuf,
    },
    Export {
        /// Fontgarden package path to export from.
        #[clap(parse(from_os_str), value_name = "FONTGARDEN_PATH")]
        fontgarden_path: PathBuf,

        /// Sets to export glyphs from, in addition to the default set.
        #[clap(long)]
        set_names: Vec<Name>,

        /// Alternatively, a text file of glyphs to export, one per line.
        #[clap(long, parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyph_names_file: Option<PathBuf>,

        /// Sources to export glyphs for [default: all]
        #[clap(long)]
        source_names: Vec<Name>,

        /// Directory to export into [default: current dir].
        #[clap(long, parse(from_os_str))]
        output_dir: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::New { path } => {
            let fontgarden = structs::Fontgarden::new();
            fontgarden.save(path);
        }
        Commands::Import {
            fontgarden_path,
            glyph_names_file,
            set_name,
            source_name,
            font,
        } => {
            let mut fontgarden =
                structs::Fontgarden::from_path(fontgarden_path).expect("can't load fontgarden");
            let import_glyphs =
                structs::load_glyph_list(glyph_names_file).expect("can't load glyphs file");
            let font = norad::Font::load(&font).expect("can't load font");
            let source_name_font = font
                .font_info
                .style_name
                .as_ref()
                .map(|v| Name::new(v).unwrap());
            let source_name = source_name
                .as_ref()
                .or(source_name_font.as_ref())
                .expect("can't determine source name to import into");
            fontgarden
                .import(&font, &import_glyphs, set_name, source_name)
                .expect("can't import font");
            fontgarden.save(fontgarden_path)
        }
        Commands::Export {
            fontgarden_path,
            set_names,
            glyph_names_file,
            source_names,
            output_dir,
        } => {
            let fontgarden =
                structs::Fontgarden::from_path(fontgarden_path).expect("can't load fontgarden");
            let glyph_names = match glyph_names_file {
                Some(path) => structs::load_glyph_list(path).expect("can't load glyph names"),
                None => {
                    let mut names = HashSet::new();
                    for set in fontgarden.sets.values() {
                        names.extend(set.glyph_coverage().iter().cloned());
                    }
                    names
                }
            };

            let set_names: HashSet<Name> = if set_names.is_empty() {
                fontgarden.sets.keys().cloned().collect()
            } else {
                set_names.iter().cloned().collect()
            };

            let source_names: HashSet<Name> = if source_names.is_empty() {
                let mut names = HashSet::new();
                for set in fontgarden.sets.values() {
                    names.extend(set.sources.keys().cloned());
                }
                names
            } else {
                source_names.iter().cloned().collect()
            };

            let ufos = fontgarden
                .export(&set_names, &glyph_names, &source_names)
                .expect("can't export to ufos");

            let output_dir = match output_dir {
                Some(d) => d.clone(),
                None => std::env::current_dir().expect("can't get current dir"),
            };
            for (ufo_name, ufo) in ufos.iter() {
                let filename = format!("{ufo_name}.ufo");
                ufo.save(output_dir.join(filename)).expect("can't save ufo");
            }
        }
    }
}
