use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{ArgGroup, CommandFactory, Parser, Subcommand};
use norad::Name;
use structs::Fontgarden;

mod errors;
mod structs;
mod util;

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
        #[clap(parse(from_os_str))]
        fontgarden_path: PathBuf,

        /// Text file of glyphs to import, one per line. Use multiple times.
        #[clap(long = "glyphs-file", parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyphs_files: Vec<PathBuf>,

        /// Set to import glyphs into. Use multiple times.
        #[clap(long = "set", value_name = "NAME")]
        sets: Vec<Name>,

        /// Unified Font Object (UFO) to import from.
        #[clap(parse(from_os_str), value_name = "UFOS")]
        fonts: Vec<PathBuf>,
        //
        // TODO:
        // /// Set the source name of the font to be imported.
        // #[clap(long = "source-name", value_name = "SOURCE_NAME")]
        // source_names: Vec<Name>,
    },
    #[clap(group(
        ArgGroup::new("glyph_names")
            .required(true)
            .args(&["sets", "glyphs-file"]),
    ))]
    Export {
        /// Fontgarden package path to export from.
        #[clap(parse(from_os_str))]
        fontgarden_path: PathBuf,

        /// Sets to export glyphs from. Use multiple times.
        #[clap(long = "set", value_name = "NAME")]
        sets: Vec<Name>,

        /// Alternatively, a text file of glyphs to export, one per line.
        #[clap(long, parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyphs_file: Option<PathBuf>,

        /// Sources to export glyphs for [default: all]
        #[clap(long = "source-name", value_name = "SOURCE_NAME")]
        source_names: Vec<Name>,

        /// Directory to export into [default: current dir].
        #[clap(long, parse(from_os_str))]
        output_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::New { path } => {
            new(path)?;
        }
        Commands::Import {
            fontgarden_path,
            glyphs_files,
            sets,
            fonts,
        } => {
            import(glyphs_files, sets, fontgarden_path, fonts)?;
        }
        Commands::Export {
            fontgarden_path,
            sets,
            glyphs_file,
            source_names,
            output_dir,
        } => {
            export(
                fontgarden_path,
                glyphs_file.as_ref().map(|f| f.as_ref()),
                sets,
                source_names,
                output_dir.as_ref(),
            )?;
        }
    }

    Ok(())
}

fn new(path: &Path) -> Result<()> {
    let fontgarden = Fontgarden::new();
    fontgarden.save(path)?;

    Ok(())
}

fn import(
    glyphs_files: &[PathBuf],
    sets: &[Name],
    fontgarden_path: &Path,
    fonts: &[PathBuf],
) -> Result<()> {
    if !glyphs_files.is_empty() && glyphs_files.len() != sets.len() {
        error_and_exit(
            clap::ErrorKind::WrongNumberOfValues,
            "The --glyphs-file argument must occur as often as the --set argument.",
        );
    }

    let mut fontgarden = Fontgarden::from_path(fontgarden_path).context("can't load fontgarden")?;

    let mut set_members = Vec::new();
    if !glyphs_files.is_empty() {
        // If glyph name files are specified, take the glyph names to
        // import from them. An unknown set name means a new set should
        // be created with the given glyph names.
        for (set_name, glyphs_file) in sets.iter().zip(glyphs_files.iter()) {
            let glyph_names = util::load_glyph_list(glyphs_file).expect("can't load glyphs file");
            set_members.push((set_name.clone(), glyph_names));
        }
    } else {
        // When only set names are specified, take the glyph names to
        // import from the Fontgarden itself. An unknown set name is
        // therefore an error.
        // TODO: Should we then take all the glyph names found in all UFOs?
        for set_name in sets.iter() {
            match fontgarden.sets.get(set_name) {
                Some(set) => {
                    let coverage = set.glyph_coverage();
                    set_members.push((set_name.clone(), coverage));
                }
                None => {
                    error_and_exit(
                        clap::ErrorKind::ValueValidation,
                        format!("Cannot find set named '{}'. To define a new set, use the --glyphs-file argument.", set_name),
                    );
                }
            }
        }
    }

    for font in fonts {
        let font = norad::Font::load(&font).expect("can't load font");
        let source_name = util::guess_source_name(&font)
            .expect("need a styleName in the UFO to derive a source name from");

        for (set_name, import_glyphs) in &set_members {
            fontgarden
                .import(&font, import_glyphs, set_name, &source_name)
                .expect("can't import font")
        }
    }

    fontgarden.save(fontgarden_path)?;

    Ok(())
}

fn export(
    fontgarden_path: &Path,
    glyphs_file: Option<&Path>,
    sets: &[Name],
    source_names: &[Name],
    output_dir: Option<&PathBuf>,
) -> Result<()> {
    let fontgarden = Fontgarden::from_path(fontgarden_path).context("can't load fontgarden")?;

    let coverage: HashMap<Name, HashSet<Name>> = fontgarden
        .sets
        .iter()
        .map(|(name, set)| (name.clone(), set.glyph_coverage()))
        .collect();
    let mut reverse_coverage = HashMap::new();
    for (set_name, coverage) in &coverage {
        for glyph_name in coverage {
            reverse_coverage.insert(glyph_name.clone(), set_name.clone());
        }
    }

    // NOTE: export's --set and --glyphs-file behave differently from
    // import. You either have a glyphs file with the stuff you want to
    // export or the names of sets.
    let glyph_names = match glyphs_file {
        Some(path) => crate::util::load_glyph_list(path).expect("can't load glyph names"),
        None => {
            let mut names = HashSet::new();

            if sets.is_empty() {
                for set_name in fontgarden.sets.keys() {
                    names.extend(coverage[set_name].iter().cloned());
                }
            } else {
                for set_name in sets {
                    let coverage = coverage
                        .get(set_name)
                        .unwrap_or_else(|| panic!("cannot find set named {}", set_name));
                    names.extend(coverage.iter().cloned());
                }
            }

            names
        }
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
        .export(&glyph_names, &source_names)
        .expect("can't export to ufos");
    let output_dir = match output_dir {
        Some(d) => d.clone(),
        None => std::env::current_dir().expect("can't get current dir"),
    };
    for (ufo_name, ufo) in ufos.iter() {
        let filename = format!("{ufo_name}.ufo");
        ufo.save(output_dir.join(filename)).expect("can't save ufo");
    }

    Ok(())
}

fn error_and_exit(kind: clap::ErrorKind, message: impl std::fmt::Display) -> ! {
    let mut cmd = Cli::command();
    cmd.error(kind, message).exit();
}
