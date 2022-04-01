use norad::Name;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("failed to load data from disk")]
    Io(#[from] std::io::Error),
    #[error("a fontgarden must be a directory")]
    NotAFontgarden,
    #[error("cannot import a glyph as it's in a different set already")]
    DuplicateGlyph,
    #[error("no default layer for source found")]
    NoDefaultLayer,
}

#[derive(Error, Debug)]
pub enum SaveError {
    #[error("failed to remove target directory before overwriting")]
    Cleanup(#[source] std::io::Error),
    #[error("failed to create target fontgarden directory")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to save set '{0}'")]
    SaveSet(Name, #[source] SaveSetError),
}

#[derive(Error, Debug)]
pub enum SaveSetError {
    #[error("failed to create set directory")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to write the set's glyph_data.csv file")]
    WriteGlyphData(#[source] csv::Error),
    #[error("failed to save source '{0}'")]
    SaveSource(Name, #[source] SaveSourceError),
}

#[derive(Error, Debug)]
pub enum SaveSourceError {
    #[error("failed to create source directory")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to save layer '{0}'")]
    SaveLayer(Name, #[source] SaveLayerError),
}

#[derive(Error, Debug)]
pub enum SaveLayerError {
    #[error("failed to create layer directory")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to write the layer's layerinfo.plist file")]
    WriteLayerInfo(#[source] plist::Error),
    #[error("failed to write the layer's color_marks.csv file")]
    WriteColorMarks(#[source] csv::Error),
    #[error("failed to save glyph '{0}'")]
    SaveGlyph(Name, #[source] norad::error::GlifWriteError),
}

#[derive(Error, Debug)]
pub enum ExportError {
    #[error("failed to load data from disk")]
    Other(#[from] Box<dyn std::error::Error>),
}
