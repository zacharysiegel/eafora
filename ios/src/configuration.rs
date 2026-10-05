use std::path::PathBuf;
use std::sync::OnceLock;

use shared::artifact::FilesystemArtifactCache;
use shared::license::DistributionContext;

use crate::distribution::FfiDistributionContext;
use crate::error::FfiError;

static CONFIGURATION: OnceLock<Configuration> = OnceLock::new();

struct Configuration {
    cache_directory: PathBuf,
    distribution_context: DistributionContext,
}

#[uniffi::export]
pub fn configure(cache_directory: String, distribution_context: FfiDistributionContext) -> Result<(), FfiError> {
    let configuration: Configuration = Configuration {
        cache_directory: PathBuf::from(cache_directory),
        distribution_context: DistributionContext::from(distribution_context),
    };

    set_once(&CONFIGURATION, configuration)
}

fn set_once(cell: &OnceLock<Configuration>, configuration: Configuration) -> Result<(), FfiError> {
    let set_result: Result<(), Configuration> = cell.set(configuration);

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_configuration) => Err(FfiError::Failed {
            message: "the configuration is already set".to_string(),
        }),
    }
}

fn get_configuration() -> Result<&'static Configuration, FfiError> {
    let configuration: Option<&'static Configuration> = CONFIGURATION.get();

    let Some(configuration) = configuration
    else {
        return Err(FfiError::Failed {
            message: "the configuration has not been set".to_string(),
        });
    };

    Ok(configuration)
}

pub fn create_cache() -> Result<FilesystemArtifactCache, FfiError> {
    let configuration: &Configuration = get_configuration()?;

    Ok(FilesystemArtifactCache::create(configuration.cache_directory.clone()))
}

pub fn get_distribution_context() -> Result<DistributionContext, FfiError> {
    let configuration: &Configuration = get_configuration()?;

    Ok(configuration.distribution_context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_configuration(cache_directory: &str) -> Configuration {
        Configuration {
            cache_directory: PathBuf::from(cache_directory),
            distribution_context: DistributionContext::FirstParty,
        }
    }

    #[test]
    fn set_once_accepts_the_first_configuration_and_rejects_a_second() {
        let cell: OnceLock<Configuration> = OnceLock::new();

        let first: Result<(), FfiError> = set_once(&cell, create_configuration("/caches/first"));
        let second: Result<(), FfiError> = set_once(&cell, create_configuration("/caches/second"));

        assert!(first.is_ok());
        assert!(second.is_err());
        assert_eq!(cell.get().unwrap().cache_directory, PathBuf::from("/caches/first"));
    }
}
