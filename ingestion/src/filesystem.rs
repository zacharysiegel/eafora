use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct FileReference {
    pub path: PathBuf,
    pub byte_count: u64,
}

#[derive(Debug, Clone)]
pub struct Hashed<T> {
    inner: T,
    sha256_hex: String,
}

impl<T> Hashed<T> {
    pub fn new(inner: T, bytes: impl AsRef<[u8]>) -> Self {
        Hashed {
            inner,
            sha256_hex: sha256_hex(bytes.as_ref()),
        }
    }

    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }

    pub fn new_with_sha(inner: T, sha256_hex: String) -> Self {
        Hashed { inner, sha256_hex }
    }
}

impl<T> Deref for Hashed<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.inner }
}

pub fn filename_of(path: &Path) -> Result<&str, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::from(format!("path missing filename component: {:?}", path)))
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, AppError> {
    fs::read(path).map_err(|err| AppError::from(format!("read {:?}: {}", path, err)))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher: Sha256 = Sha256::new();
    hasher.update(bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    hex_encode(&digest)
}

pub fn sha256_hex_of_file(path: &Path) -> Result<String, AppError> {
    let bytes: Vec<u8> = read_bytes(path)?;
    Ok(sha256_hex(&bytes))
}

/// Read a file at `<base_dir>/<relative_path>`, hash its bytes, and verify that
/// the computed sha256 matches `expected_sha256_hex`. Used by readers that
/// validate a manifest's referenced files against their recorded hashes.
pub fn load_hashed_file(
    base_dir: &Path,
    relative_path: &str,
    expected_sha256_hex: &str,
) -> Result<Hashed<FileReference>, AppError> {
    let path: PathBuf = base_dir.join(relative_path);
    let bytes: Vec<u8> = read_bytes(&path)?;
    let hashed: Hashed<FileReference> = Hashed::new(
        FileReference { path: path.clone(), byte_count: bytes.len() as u64 },
        &bytes,
    );

    if hashed.sha256_hex() != expected_sha256_hex {
        return Err(AppError::from(format!(
            "sha256 mismatch for {:?}: expected {}, file hashes to {}",
            path, expected_sha256_hex, hashed.sha256_hex(),
        )));
    }

    Ok(hashed)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut hex_string: String = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_string.push_str(&format!("{:02x}", byte));
    }
    hex_string
}
