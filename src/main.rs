use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod lib;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
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
        path: PathBuf,

        /// Text file of glyphs to import, one per line.
        #[clap(parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyph_names_file: PathBuf,

        /// Set to import glyphs into.
        #[clap(long, default_value = "default")]
        set_name: String,

        /// Source to import glyphs into [default: infer from style name].
        #[clap(long)]
        source_name: Option<String>,

        /// Unified Font Object (UFO) to import from.
        #[clap(parse(from_os_str))]
        font: PathBuf,
    },
    Export {
        /// Fontgarden package path to export from.
        #[clap(parse(from_os_str), value_name = "FONTGARDEN_PATH")]
        path: PathBuf,

        /// Sets to export glyphs from, in addition to the default set.
        #[clap(long)]
        set_names: Vec<String>,

        /// Alternatively, a text file of glyphs to export, one per line.
        #[clap(parse(from_os_str), value_name = "GLYPHS_FILE")]
        glyph_names_file: Option<PathBuf>,

        /// Sources to export glyphs for [default: all]
        #[clap(long)]
        source_names: Vec<String>,

        /// Directory to export into [default: current dir].
        #[clap(parse(from_os_str))]
        output_dir: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::New { path } => {
            let fontgarden = lib::Fontgarden::new();
            fontgarden.save(path);
        }
        Commands::Import {
            path,
            glyph_names_file,
            set_name,
            source_name,
            font,
        } => {
            todo!()
        }
        Commands::Export {
            path,
            set_names,
            glyph_names_file,
            source_names,
            output_dir,
        } => {
            todo!()
        }
    }
}
