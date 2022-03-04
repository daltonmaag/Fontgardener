use std::collections::HashMap;
use std::fs::{create_dir, read_dir};
use std::path::Path;

use norad::Name;

use super::layer::Layer;
use super::LoadError;

#[derive(Debug, Default, PartialEq)]
pub struct Source {
    pub layers: HashMap<Name, Layer>,
}

impl Source {
    pub(crate) fn from_path(path: &Path) -> Result<Self, LoadError> {
        let mut layers = HashMap::new();
        let mut found_default = false;

        for entry in read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir()
                && path
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with("glyphs."))
                    .unwrap_or(false)
            {
                let (layer, layerinfo) = Layer::from_path(&path)?;
                if layerinfo.default {
                    if !found_default {
                        found_default = true;
                    } else {
                        return Err(LoadError::DuplicateDefaultLayer);
                    }
                }
                layers.insert(layerinfo.name, layer);
            }
        }

        if !found_default {
            return Err(LoadError::NoDefaultLayer);
        }

        Ok(Source { layers })
    }

    pub(crate) fn save(&self, source_name: &str, set_path: &Path) {
        let source_path = set_path.join(format!("source.{source_name}"));
        create_dir(&source_path).expect("can't create source dir");
        for (layer_name, layer) in &self.layers {
            layer.save(layer_name, &source_path);
        }
    }
}
