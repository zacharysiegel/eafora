use std::path::PathBuf;
use std::sync::OnceLock;

use shared::artifact::FilesystemArtifactCache;
use shared::license::DistributionContext;

use crate::distribution::FfiDistributionContext;
use crate::error::FfiError;

static HOST_ENVIRONMENT: OnceLock<HostEnvironment> = OnceLock::new();

struct HostEnvironment {
    cache_directory: PathBuf,
    distribution_context: DistributionContext,
}

#[uniffi::export]
pub fn set_host_environment(cache_directory: String, distribution_context: FfiDistributionContext) -> Result<(), FfiError> {
    let host_environment: HostEnvironment = HostEnvironment {
        cache_directory: PathBuf::from(cache_directory),
        distribution_context: DistributionContext::from(distribution_context),
    };

    set_once(&HOST_ENVIRONMENT, host_environment)
}

fn set_once(cell: &OnceLock<HostEnvironment>, host_environment: HostEnvironment) -> Result<(), FfiError> {
    let set_result: Result<(), HostEnvironment> = cell.set(host_environment);

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_host_environment) => Err(FfiError::Failed {
            message: "the host environment is already set".to_string(),
        }),
    }
}

fn get_host_environment() -> Result<&'static HostEnvironment, FfiError> {
    let host_environment: Option<&'static HostEnvironment> = HOST_ENVIRONMENT.get();

    let Some(host_environment) = host_environment
    else {
        return Err(FfiError::Failed {
            message: "the host environment has not been set".to_string(),
        });
    };

    Ok(host_environment)
}

pub fn create_cache() -> Result<FilesystemArtifactCache, FfiError> {
    let host_environment: &HostEnvironment = get_host_environment()?;

    Ok(FilesystemArtifactCache::create(host_environment.cache_directory.clone()))
}

pub fn get_distribution_context() -> Result<DistributionContext, FfiError> {
    let host_environment: &HostEnvironment = get_host_environment()?;

    Ok(host_environment.distribution_context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_host_environment(cache_directory: &str) -> HostEnvironment {
        HostEnvironment {
            cache_directory: PathBuf::from(cache_directory),
            distribution_context: DistributionContext::FirstParty,
        }
    }

    #[test]
    fn set_once_accepts_the_first_host_environment_and_rejects_a_second() {
        let cell: OnceLock<HostEnvironment> = OnceLock::new();

        let first: Result<(), FfiError> = set_once(&cell, create_host_environment("/caches/first"));
        let second: Result<(), FfiError> = set_once(&cell, create_host_environment("/caches/second"));

        assert!(first.is_ok());
        assert!(second.is_err());
        assert_eq!(cell.get().unwrap().cache_directory, PathBuf::from("/caches/first"));
    }
}
