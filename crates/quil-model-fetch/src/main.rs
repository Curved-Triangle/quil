#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{Parser, Subcommand};
use quil_core::{CatalogError, ModelFile, ModelSpec, default_model_dir, embedded_catalog};
use reqwest::{blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(name = "quil-model-fetch", version, about)]
struct Cli {
    /// Override Quil's per-user model directory.
    #[arg(long, global = true, value_name = "PATH")]
    model_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List catalog entries without making a network request.
    List,
    /// Explicitly download and checksum-install a model.
    Download {
        model: String,
        /// Replace an existing file only after its replacement verifies.
        #[arg(long)]
        force: bool,
    },
    /// Verify installed files against the checked-in catalog.
    Verify { model: String },
}

#[derive(Debug, Error)]
enum FetchError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("model directory is unavailable; set QUIL's --model-dir explicitly")]
    ModelDirectory,
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("download failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{path} has {actual} bytes; catalog requires {expected}")]
    SizeMismatch {
        path: PathBuf,
        expected: u64,
        actual: u64,
    },
    #[error("SHA-256 mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Invalid(String),
}

fn main() -> Result<(), FetchError> {
    let cli = Cli::parse();
    let catalog = embedded_catalog()?;
    let model_dir = match cli.model_dir {
        Some(path) => path,
        None => default_model_dir().ok_or(FetchError::ModelDirectory)?,
    };

    match cli.command {
        Command::List => {
            for model in &catalog.models {
                let bytes = model.files.iter().map(|file| file.bytes).sum::<u64>();
                println!(
                    "{}\t{}\t{}\t{} bytes",
                    model.id, model.name, model.license, bytes
                );
            }
        }
        Command::Download { model, force } => {
            let model = catalog.model(&model)?;
            let client = Client::builder()
                .https_only(true)
                .redirect(Policy::limited(5))
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30 * 60))
                .user_agent(concat!("quil-model-fetch/", env!("CARGO_PKG_VERSION")))
                .build()?;
            download_model(&client, model, &model_dir, force)?;
            println!(
                "Installed {} in {}",
                model.id,
                model_dir.join(&model.id).display()
            );
        }
        Command::Verify { model } => {
            let model = catalog.model(&model)?;
            verify_model(model, &model_dir)?;
            println!("Verified {}", model.id);
        }
    }
    Ok(())
}

fn download_model(
    client: &Client,
    model: &ModelSpec,
    model_dir: &Path,
    force: bool,
) -> Result<(), FetchError> {
    for file in &model.files {
        let destination = model_dir.join(&model.id).join(&file.path);
        if destination.exists() && !force {
            verify_file(&destination, file)?;
            println!("Already verified {}", destination.display());
            continue;
        }
        download_file(client, file, &destination)?;
        println!("Verified and installed {}", destination.display());
    }
    Ok(())
}

fn verify_model(model: &ModelSpec, model_dir: &Path) -> Result<(), FetchError> {
    for file in &model.files {
        let path = model_dir.join(&model.id).join(&file.path);
        verify_file(&path, file)?;
    }
    Ok(())
}

fn download_file(client: &Client, file: &ModelFile, destination: &Path) -> Result<(), FetchError> {
    let parent = destination.parent().ok_or_else(|| {
        FetchError::Invalid(format!("{} has no parent directory", destination.display()))
    })?;
    fs::create_dir_all(parent).map_err(|source| FetchError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = parent.join(format!(".quil-download-{}", std::process::id()));

    let result = (|| {
        let response = client.get(&file.url).send()?.error_for_status()?;
        if let Some(length) = response.content_length()
            && length != file.bytes
        {
            return Err(FetchError::Invalid(format!(
                "server announced {length} bytes for {}; catalog requires {}",
                file.url, file.bytes
            )));
        }

        let mut output = File::create_new(&temporary).map_err(|source| FetchError::Io {
            path: temporary.clone(),
            source,
        })?;
        let mut input = response;
        let mut buffer = vec![0_u8; 1024 * 1024];
        let mut hasher = Sha256::new();
        let mut written = 0_u64;
        loop {
            let count = input.read(&mut buffer).map_err(|source| FetchError::Io {
                path: temporary.clone(),
                source,
            })?;
            if count == 0 {
                break;
            }
            written = written.saturating_add(count as u64);
            if written > file.bytes {
                return Err(FetchError::Invalid(format!(
                    "download exceeded the catalog size for {}",
                    file.url
                )));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|source| FetchError::Io {
                    path: temporary.clone(),
                    source,
                })?;
            hasher.update(&buffer[..count]);
        }
        output.flush().map_err(|source| FetchError::Io {
            path: temporary.clone(),
            source,
        })?;
        output.sync_all().map_err(|source| FetchError::Io {
            path: temporary.clone(),
            source,
        })?;
        drop(output);

        verify_digest(&temporary, file, written, finish_hex(hasher))?;
        fs::rename(&temporary, destination).map_err(|source| FetchError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn verify_file(path: &Path, file: &ModelFile) -> Result<(), FetchError> {
    let mut input = File::open(path).map_err(|source| FetchError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let length = input
        .metadata()
        .map_err(|source| FetchError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if length != file.bytes {
        return Err(FetchError::SizeMismatch {
            path: path.to_path_buf(),
            expected: file.bytes,
            actual: length,
        });
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut read = 0_u64;
    loop {
        let count = input.read(&mut buffer).map_err(|source| FetchError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        read = read.saturating_add(count as u64);
        hasher.update(&buffer[..count]);
    }
    verify_digest(path, file, read, finish_hex(hasher))
}

fn finish_hex(hasher: Sha256) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn verify_digest(
    path: &Path,
    file: &ModelFile,
    actual_bytes: u64,
    actual_sha256: String,
) -> Result<(), FetchError> {
    if actual_bytes != file.bytes {
        return Err(FetchError::SizeMismatch {
            path: path.to_path_buf(),
            expected: file.bytes,
            actual: actual_bytes,
        });
    }
    if actual_sha256 != file.sha256 {
        return Err(FetchError::ChecksumMismatch {
            path: path.to_path_buf(),
            expected: file.sha256.clone(),
            actual: actual_sha256,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sha256_vector_passes() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        assert_eq!(
            finish_hex(hasher),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let file = ModelFile {
            url: "https://example.com/model".into(),
            path: "model.bin".into(),
            sha256: "a".repeat(64),
            bytes: 3,
        };
        assert!(verify_digest(Path::new("model.bin"), &file, 3, "b".repeat(64)).is_err());
    }
}
