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
pub const SECRET_R2_ACCESS_KEY_ID: &str = "r2_access_key_id";
pub const SECRET_R2_SECRET_ACCESS_KEY: &str = "r2_secret_access_key";

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
            .map_err(|err| AppError::from(format!("put_object bucket={} key={}: {}", self.bucket, key, err)))?;

        Ok(())
    }

    fn url(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}
