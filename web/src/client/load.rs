use shared::AppError;
use shared::artifact::{ArtifactCache, Bundle, Manifest, manifest};
use shared::filesystem;
use shared::license::DistributionContext;

use crate::client::cache::OpfsArtifactCache;
use crate::client::fetch;

const EMBEDDED_BASE_URL: &str = "/embedded_artifacts";

pub async fn load_embedded_bundle(cache: &OpfsArtifactCache) -> Result<Bundle, AppError> {
    let manifest_url: String = format!("{EMBEDDED_BASE_URL}/{}", manifest::MANIFEST_FILENAME);
    let manifest_bytes: Vec<u8> = fetch::fetch_bytes(&manifest_url).await?;
    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, &manifest_bytes).await?;

    for entry in manifest.file_entries() {
        let file_url: String = format!("{EMBEDDED_BASE_URL}/{}", entry.relative_path);
        let file_bytes: Vec<u8> = fetch::fetch_bytes(&file_url).await?;
        filesystem::verify_sha256(&file_bytes, &entry.sha256)?;
        cache.put(&manifest.version, &entry.relative_path, &file_bytes).await?;
    }

    Bundle::open(cache, &manifest.version, DistributionContext::Embedded).await
}
