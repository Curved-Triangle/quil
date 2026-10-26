#![forbid(unsafe_code)]

mod moonshine;
mod moonshine_tokenizer;
mod whisper;

use std::path::{Path, PathBuf};

use quil_core::{Backend, ModelSpec, Transcriber};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    Auto,
    Metal,
    CoreMl,
    Cpu,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("model artifact {path} is invalid: {message}; run quil-model-fetch download/verify")]
    Artifact { path: PathBuf, message: String },
    #[error("{backend} backend is unavailable: {message}")]
    Unavailable {
        backend: &'static str,
        message: String,
    },
    #[error("failed to initialize {backend}: {message}")]
    Initialization {
        backend: &'static str,
        message: String,
    },
}

pub struct LoadedModel {
    pub transcriber: Box<dyn Transcriber>,
    pub execution: String,
}

pub fn load(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
) -> Result<LoadedModel, InferenceError> {
    verify_artifacts(model, model_dir)?;
    match model.backend {
        Backend::Whisper => whisper::load(model, model_dir, execution),
        Backend::Moonshine => moonshine::load(model, model_dir, execution),
    }
}

fn verify_artifacts(model: &ModelSpec, model_dir: &Path) -> Result<(), InferenceError> {
    for artifact in &model.files {
        let path = model_dir.join(&artifact.path);
        let metadata = std::fs::metadata(&path).map_err(|error| InferenceError::Artifact {
            path: path.clone(),
            message: error.to_string(),
        })?;
        if metadata.len() != artifact.bytes {
            return Err(InferenceError::Artifact {
                path,
                message: format!(
                    "expected {} bytes, found {}; run quil-model-fetch download",
                    artifact.bytes,
                    metadata.len()
                ),
            });
        }
        // The installer verifies these files, but the runtime must never trust
        // them afterwards: a model file swapped or corrupted after install is
        // the largest untrusted-input surface Quil feeds to a native parser.
        // Never rely on the byte length alone as proof of integrity.
        let digest = sha256_of_file(&path).map_err(|message| InferenceError::Artifact {
            path: path.clone(),
            message,
        })?;
        if digest != artifact.sha256 {
            return Err(InferenceError::Artifact {
                path,
                message: format!(
                    "expected SHA-256 {}, found {}; the artifact is corrupted or was replaced, \
                     run quil-model-fetch download --force",
                    artifact.sha256, digest
                ),
            });
        }
    }
    Ok(())
}

fn sha256_of_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        use std::io::Read;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_lower(hasher.finalize()))
}

fn hex_lower(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = digest.as_ref();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(crate) fn initialization_error(
    backend: &'static str,
    error: impl std::fmt::Display,
) -> InferenceError {
    InferenceError::Initialization {
        backend,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn artifact_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quil-verify-artifacts-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create artifact test directory");
        dir
    }

    fn model_spec(sha256: &str, bytes: u64) -> ModelSpec {
        ModelSpec {
            id: "audit-model".into(),
            name: "Audit model".into(),
            description: "test only".into(),
            backend: Backend::Moonshine,
            revision: "test".into(),
            languages: vec!["en".into()],
            license: "MIT".into(),
            license_url: "https://example.com/license".into(),
            default: false,
            files: vec![quil_core::ModelFile {
                url: "https://example.com/model.bin".into(),
                path: "model.bin".into(),
                sha256: sha256.into(),
                bytes,
            }],
        }
    }

    // SHA-256("abc")
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn accepts_an_artifact_matching_length_and_digest() {
        let dir = artifact_dir("accepts");
        std::fs::write(dir.join("model.bin"), b"abc").expect("write artifact");
        let model = model_spec(ABC_SHA256, 3);
        assert!(verify_artifacts(&model, &dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_tampered_artifact_with_matching_length() {
        let dir = artifact_dir("tampered");
        std::fs::write(dir.join("model.bin"), b"abd").expect("write artifact");
        let model = model_spec(ABC_SHA256, 3);
        let error = verify_artifacts(&model, &dir).expect_err("tampered artifact must be rejected");
        assert!(format!("{error}").contains("expected SHA-256"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_an_artifact_with_the_wrong_length() {
        let dir = artifact_dir("wrong-length");
        std::fs::write(dir.join("model.bin"), b"abcd").expect("write artifact");
        let model = model_spec(ABC_SHA256, 3);
        let error = verify_artifacts(&model, &dir).expect_err("wrong length must be rejected");
        assert!(format!("{error}").contains("expected 3 bytes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_missing_artifact() {
        let dir = artifact_dir("missing");
        let model = model_spec(ABC_SHA256, 3);
        assert!(verify_artifacts(&model, &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_encoding_matches_the_reference_vector() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        assert_eq!(hex_lower(hasher.finalize()), ABC_SHA256);
    }
}
