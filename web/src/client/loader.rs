use shared::AppError;
use shared::artifact::{ArtifactCache, Bundle, Manifest, ManifestEntry, manifest};
use shared::filesystem;
use shared::license::DistributionContext;

use crate::client::cache::OpfsArtifactCache;
use crate::client::fetch;

const EMBEDDED_BASE_URL: &str = "/embedded_artifacts";

/// Loads the embedded bundle for first paint: fetch its manifest and every referenced file from the
/// same-origin `embedded_artifacts/` static assets, verify each against the manifest's SHA-256, write
/// it through the cache, then open the bundle. The live-CDN fetch, discovery, and hot-swap are Phase D.
pub async fn load_embedded_bundle(cache: &OpfsArtifactCache) -> Result<Bundle, AppError> {
    let manifest_url: String = format!("{EMBEDDED_BASE_URL}/{}", manifest::MANIFEST_FILENAME);
    let manifest_bytes: Vec<u8> = fetch::fetch_bytes(&manifest_url).await?;
    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;
    log::debug!("embedded manifest parsed [version={} manifest_bytes={}]", manifest.version, manifest_bytes.len());

    let version_label: &str = &manifest.version;
    cache.put(version_label, manifest::MANIFEST_FILENAME, &manifest_bytes).await?;

    for entry in manifest_file_entries(&manifest) {
        let file_url: String = format!("{EMBEDDED_BASE_URL}/{}", entry.relative_path);
        let file_bytes: Vec<u8> = fetch::fetch_bytes(&file_url).await?;
        filesystem::verify_sha256(&file_bytes, &entry.sha256)?;
        cache.put(version_label, &entry.relative_path, &file_bytes).await?;
    }

    log::debug!("opening embedded bundle from cache [version={version_label}]");
    Bundle::open(cache, version_label, DistributionContext::Embedded).await
}

/// The geometry entry followed by every statistic shard entry. `Bundle::open` filters to the
/// authorized shard classes on its side; the downsampled embedded bundle ships Base shards only.
fn manifest_file_entries(manifest: &Manifest) -> impl Iterator<Item = &ManifestEntry> {
    std::iter::once(&manifest.geometry)
        .chain(manifest.statistics.values().flat_map(|by_license_class| by_license_class.values()))
}
