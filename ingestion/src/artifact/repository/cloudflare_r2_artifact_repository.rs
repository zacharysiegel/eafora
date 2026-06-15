use std::error::Error;
use std::path::Path;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

const R2_REGION_PLACEHOLDER: &str = "auto";
const R2_CREDENTIALS_PROVIDER_NAME: &str = "eafora-r2";

pub const ENV_R2_ACCOUNT_ID: &str = "R2_ACCOUNT_ID";
pub const ENV_R2_ARTIFACT_BUCKET: &str = "R2_ARTIFACT_BUCKET";
pub const ENV_R2_ARTIFACT_PUBLIC_BASE_URL: &str = "R2_ARTIFACT_PUBLIC_BASE_URL";
pub const SECRET_R2_PUBLISH_TOKEN: &str = "cloudflare.r2.publish.token";
pub const SECRET_R2_PUBLISH_SECRET_ACCESS_KEY: &str = "cloudflare.r2.publish.secret_access_key";

pub struct CloudflareR2Config {
    pub account_id: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub public_base_url: String,
}

pub struct CloudflareR2ArtifactRepository {
    client: Client,
    bucket: String,
    public_base_url: String,
}

impl CloudflareR2ArtifactRepository {
    pub async fn create(config: CloudflareR2Config) -> Result<Self, AppError> {
        let credentials: Credentials = Credentials::new(
            config.access_key_id,
            config.secret_access_key,
            None,
            None,
            R2_CREDENTIALS_PROVIDER_NAME,
        );
        let endpoint_url: String = format!("https://{}.r2.cloudflarestorage.com", config.account_id);

        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .endpoint_url(endpoint_url)
            .credentials_provider(credentials)
            .region(Region::new(R2_REGION_PLACEHOLDER))
            .load()
            .await;
        let client: Client = Client::new(&sdk_config);

        Ok(CloudflareR2ArtifactRepository {
            client,
            bucket: config.bucket,
            public_base_url: config.public_base_url,
        })
    }
}

impl ArtifactRepository for CloudflareR2ArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        let body: ByteStream = ByteStream::from_path(source_path).await.map_err(|err| {
            AppError::from(format!("ByteStream::from_path {:?}: {}", source_path, err))
        })?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await
            .map_err(|err| AppError::from(format!("put_object bucket={} key={}: {}", self.bucket, key, render_error_chain(&err))))?;

        Ok(())
    }

    fn url(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}

/// Walk an error's `source()` chain and concatenate each level's Display.
/// The AWS SDK's top-level `SdkError::Display` summarizes to "service error"
/// or "dispatch failure" without the underlying detail; the source chain
/// carries the actual hyper / TLS / HTTP-status info.
fn render_error_chain(error: &dyn Error) -> String {
    let mut rendered: String = error.to_string();
    let mut next: Option<&dyn Error> = error.source();

    while let Some(source) = next {
        rendered.push_str(" -> ");
        rendered.push_str(&source.to_string());
        next = source.source();
    }

    rendered
}
