use std::collections::{BTreeMap, HashSet};
use std::fs::{create_dir, read_dir};
use std::path::Path;

use norad::Name;

use crate::errors::SaveSourceError;

use super::layer::Layer;
use super::LoadError;

#[derive(Debug, PartialEq)]
pub struct Source {
    // TODO: UFO layers are ordered, export from here will always sort order.
    // Relevant other than in testing?
    pub layers: BTreeMap<Name, Layer>,
}

impl Default for Source {
    fn default() -> Self {
        let layer = Layer {
            default: true,
            ..Default::default()
        };
        Self {
            layers: BTreeMap::from([(Name::new("public.default").unwrap(), layer)]),
        }
    }
}

impl Source {
    pub fn new_with_default_layer_name(name: Name) -> Self {
        let layer = Layer {
            default: true,
            ..Default::default()
        };
        Self {
            layers: BTreeMap::from([(name, layer)]),
        }
    }

    pub(crate) fn from_path(path: &Path) -> Result<Self, LoadError> {
        let mut layers = BTreeMap::new();
        let mut found_default = false;

        for entry in read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().expect("can't read file name");
            let metadata = entry.metadata()?;
            if metadata.is_dir()
                && (file_name == "glyphs" || file_name.to_string_lossy().starts_with("glyphs."))
            {
                let (layer, layerinfo) = Layer::from_path(&path)?;
                // All non-default layer names start with a dot after "glyphs".
                // Hope that we don't bump into filesystem case-sensitivity
                // issues.
                if file_name == "glyphs" {
                    found_default = true;
                }
                layers.insert(layerinfo.name, layer);
            }
        }

        if !found_default {
            return Err(LoadError::NoDefaultLayer);
        }

        Ok(Source { layers })
    }

    pub(crate) fn save(&self, source_name: &str, set_path: &Path) -> Result<(), SaveSourceError> {
        let source_path = set_path.join(format!("source.{source_name}"));
        create_dir(&source_path).map_err(SaveSourceError::CreateDir)?;

        let mut existing_layer_names = HashSet::new();
        for (layer_name, layer) in &self.layers {
            layer
                .save(layer_name, &source_path, &mut existing_layer_names)
                .map_err(|e| SaveSourceError::SaveLayer(layer_name.clone(), e))?;
        }

        Ok(())
    }

    pub fn get_default_layer_mut(&mut self) -> &mut Layer {
        self.layers
            .values_mut()
            .find(|layer| layer.default)
            .unwrap()
    }

    pub fn get_or_create_layer(&mut self, name: Name) -> &mut Layer {
        self.layers.entry(name).or_default()
    }
}
