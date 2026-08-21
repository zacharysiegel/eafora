use sha2::{Digest, Sha256};

use std::ops::Deref;

use crate::error::AppError;

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

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher: Sha256 = Sha256::new();
    hasher.update(bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    hex_encode(&digest)
}

pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError> {
    let computed: String = sha256_hex(bytes);

    if computed.eq_ignore_ascii_case(expected_hex) {
        return Ok(());
    }

    let expected_prefix: &str = if expected_hex.len() >= 8 { &expected_hex[..8] } else { expected_hex };
    let computed_prefix: &str = &computed[..8];

    Err(AppError::from(format!(
        "sha256 mismatch: expected {}..., got {}...",
        expected_prefix, computed_prefix,
    )))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut hex_string: String = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_string.push_str(&format!("{:02x}", byte));
    }
    hex_string
}

#[cfg(not(target_arch = "wasm32"))] // reads the local filesystem
pub use local::*;

#[cfg(not(target_arch = "wasm32"))] // reads the local filesystem
mod local {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    const WORKSPACE_MANIFEST_TABLE: &str = "[workspace]";

    #[derive(Debug, Clone)]
    pub struct FileReference {
        pub path: PathBuf,
        pub byte_count: u64,
    }

    pub fn filename_of(path: &Path) -> Result<&str, AppError> {
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::from(format!("path missing filename component: {:?}", path)))
    }

    pub fn find_workspace_root(start_directory: &Path) -> Result<PathBuf, AppError> {
        let mut directory: Option<&Path> = Some(start_directory);

        while let Some(candidate) = directory {
            // A directory with no readable manifest is the ordinary case while searching upward.
            let manifest_text: Option<String> = fs::read_to_string(candidate.join("Cargo.toml")).ok();
            let is_workspace_root: bool = manifest_text
                .is_some_and(|manifest_text| declares_workspace(&manifest_text));

            if is_workspace_root {
                return Ok(candidate.to_path_buf());
            }

            directory = candidate.parent();
        }

        Err(AppError::from(format!(
            "no Cargo.toml declaring a workspace at or above the starting directory; [start={}]",
            start_directory.display(),
        )))
    }

    pub fn resolve_workspace_relative(path: &Path) -> Result<PathBuf, AppError> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        let current_directory: PathBuf = env::current_dir()
            .map_err(|error| AppError::from(format!("could not read the current directory: {}", error)))?;

        Ok(find_workspace_root(&current_directory)?.join(path))
    }

    fn declares_workspace(manifest_text: &str) -> bool {
        manifest_text
            .lines()
            .any(|line| line.trim() == WORKSPACE_MANIFEST_TABLE)
    }

    pub fn read_bytes(path: &Path) -> Result<Vec<u8>, AppError> {
        fs::read(path).map_err(|err| AppError::from(format!("read {:?}: {}", path, err)))
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn declares_workspace_accepts_a_workspace_root() {
            let manifest_text: &str = "[workspace]\nresolver = \"2\"\nmembers = [\"shared\"]\n";

            assert!(declares_workspace(manifest_text));
        }

        #[test]
        fn declares_workspace_rejects_a_member_manifest() {
            let manifest_text: &str = "[package]\nname = \"shared\"\nedition.workspace = true\n";

            assert!(!declares_workspace(manifest_text));
        }

        /// A member manifest inherits from the workspace without declaring one itself.
        #[test]
        fn declares_workspace_rejects_a_manifest_naming_only_a_workspace_subtable() {
            let manifest_text: &str = "[package]\nname = \"shared\"\n\n[dependencies]\ntokio = { workspace = true }\n";

            assert!(!declares_workspace(manifest_text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BYTES: &[u8] = b"eafora fertility atlas";

    #[test]
    fn verify_sha256_accepts_matching_hash() {
        let expected_hex: String = sha256_hex(SAMPLE_BYTES);

        verify_sha256(SAMPLE_BYTES, &expected_hex).unwrap();
    }

    #[test]
    fn verify_sha256_rejects_mismatch_with_truncated_prefixes() {
        let wrong_hex: String = sha256_hex(b"a different payload");

        let error: AppError = verify_sha256(SAMPLE_BYTES, &wrong_hex).unwrap_err();

        let message: String = error.to_string();
        assert!(message.contains(&wrong_hex[..8]));
        assert!(message.contains(&sha256_hex(SAMPLE_BYTES)[..8]));
    }

    #[test]
    fn verify_sha256_is_case_insensitive() {
        let expected_hex: String = sha256_hex(SAMPLE_BYTES).to_uppercase();

        verify_sha256(SAMPLE_BYTES, &expected_hex).unwrap();
    }
}
