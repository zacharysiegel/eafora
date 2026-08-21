use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use base64::Engine;
use secr::store::SecretStore;
use secr::{cryptography, load, BASE64};
use shared::filesystem;

use crate::error::AppError;

static SECRET_STORE: LazyLock<SecretStore> = LazyLock::new(|| load_secrets().expect("loading secret store"));

pub fn master_decrypt(secret_name: &str) -> Result<Vec<u8>, AppError> {
    let master_secret_base64: String = dotenvy::var("MASTER_SECRET")?;
    let master_secret: Vec<u8> = BASE64.decode(master_secret_base64)?;
    let secret: Vec<u8> = cryptography::decrypt(&SECRET_STORE, &master_secret, secret_name)?;
    Ok(secret)
}

pub fn master_decrypt_utf8(secret_name: &str) -> Result<String, AppError> {
    let bytes: Vec<u8> = master_decrypt(secret_name)?;
    let text: String = String::from_utf8(bytes)
        .map_err(|err| AppError::from(format!("secret {:?} is not valid utf-8: {}", secret_name, err)))?;
    Ok(text)
}

/* `dotenvy` finds `.env` by searching upward from the current directory, so a relative path declared in it
   has to resolve the same way. Anchoring on the current directory would instead make the store's location
   depend on where the process was started, and Cargo starts test binaries in the package directory. */
fn load_secrets() -> Result<SecretStore, AppError> {
    let declared_store_path: String = dotenvy::var("SECR_STORE_PATH")?;
    let store_path: PathBuf = filesystem::resolve_workspace_relative(Path::new(&declared_store_path))?;
    let store: SecretStore = load::load_secrets_from_file(&store_path)?;
    Ok(store)
}
