use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const EMBEDDED_CATALOG: &str = include_str!("../../../models/catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub models: Vec<ModelSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub backend: Backend,
    pub revision: String,
    pub languages: Vec<String>,
    pub license: String,
    pub license_url: String,
    #[serde(default)]
    pub default: bool,
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Whisper,
    Moonshine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub url: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("embedded model catalog is invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("embedded model catalog is invalid: {0}")]
    Invalid(String),
    #[error("unknown model `{0}`")]
    UnknownModel(String),
}

pub fn embedded_catalog() -> Result<Catalog, CatalogError> {
    let catalog: Catalog = serde_json::from_str(EMBEDDED_CATALOG)?;
    catalog.validate()?;
    Ok(catalog)
}

impl Catalog {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != 1 {
            return Err(CatalogError::Invalid(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }

        let mut ids = HashSet::new();
        let mut defaults = 0;
        for model in &self.models {
            if !ids.insert(model.id.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "duplicate model id `{}`",
                    model.id
                )));
            }
            model.validate()?;
            defaults += usize::from(model.default);
        }
        if defaults > 1 {
            return Err(CatalogError::Invalid(
                "at most one model may be the default".into(),
            ));
        }
        Ok(())
    }

    pub fn model(&self, id: &str) -> Result<&ModelSpec, CatalogError> {
        self.models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| CatalogError::UnknownModel(id.to_owned()))
    }

    pub fn default_model(&self) -> Option<&ModelSpec> {
        self.models.iter().find(|model| model.default)
    }
}

impl ModelSpec {
    fn validate(&self) -> Result<(), CatalogError> {
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(CatalogError::Invalid(format!(
                "model id `{}` must be lowercase kebab-case",
                self.id
            )));
        }
        if self.files.is_empty() {
            return Err(CatalogError::Invalid(format!(
                "model `{}` has no files",
                self.id
            )));
        }
        if self.license.is_empty() || !self.license_url.starts_with("https://") {
            return Err(CatalogError::Invalid(format!(
                "model `{}` needs a license and HTTPS license URL",
                self.id
            )));
        }

        let mut paths = HashSet::new();
        for file in &self.files {
            if !paths.insert(file.path.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "model `{}` repeats path `{}`",
                    self.id, file.path
                )));
            }
            file.validate(&self.id)?;
        }
        Ok(())
    }
}

impl ModelFile {
    fn validate(&self, model_id: &str) -> Result<(), CatalogError> {
        if !self.url.starts_with("https://") {
            return Err(CatalogError::Invalid(format!(
                "model `{model_id}` has a non-HTTPS artifact URL"
            )));
        }
        if self.bytes == 0 {
            return Err(CatalogError::Invalid(format!(
                "model `{model_id}` file `{}` has zero length",
                self.path
            )));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CatalogError::Invalid(format!(
                "model `{model_id}` file `{}` needs a lowercase SHA-256",
                self.path
            )));
        }

        let path = Path::new(&self.path);
        let valid_path = !self.path.is_empty()
            && path.components().all(|component| {
                matches!(component, std::path::Component::Normal(_))
                    && component.as_os_str() != ".git"
            });
        if !valid_path {
            return Err(CatalogError::Invalid(format!(
                "model `{model_id}` file path `{}` is not a safe relative path",
                self.path
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid() {
        embedded_catalog().expect("checked-in model catalog must be valid");
    }

    #[test]
    fn rejects_parent_directory_artifacts() {
        let file = ModelFile {
            url: "https://example.com/model.bin".into(),
            path: "../model.bin".into(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        assert!(file.validate("test").is_err());
    }

    #[test]
    fn rejects_uppercase_hashes() {
        let file = ModelFile {
            url: "https://example.com/model.bin".into(),
            path: "model.bin".into(),
            sha256: "A".repeat(64),
            bytes: 1,
        };
        assert!(file.validate("test").is_err());
    }
}
