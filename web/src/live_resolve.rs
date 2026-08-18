use shared::artifact;
use shared::artifact::DiscoveryDocument;
use shared::AppError;

pub const DISCOVERY_PATH: &str = "/discovery";
pub const STATIC_DISCOVERY_JSON: &str = include_str!("../static/discovery");

pub fn static_discovery_document() -> Result<DiscoveryDocument, AppError> {
    artifact::parse_discovery_document(STATIC_DISCOVERY_JSON.as_bytes())
}

/* The committed document names the local repository, which is what `cargo leptos watch` serves. A deploy
   sets this variable so the value compiled in names the artifact CDN instead, which matters because the
   speculative manifest fetch that races discovery uses it. */
pub fn static_repository_base_url() -> Result<String, AppError> {
    match option_env!("EAFORA_REPOSITORY_BASE_URL") {
        Some(base_url) => Ok(base_url.to_string()),
        None => Ok(static_discovery_document()?.repository_base_url),
    }
}

pub enum AuthoritativeBase {
    Static,
    Discovered(String),
}

pub fn authoritative_repository_base(
    static_base: &str,
    discovery: Result<DiscoveryDocument, AppError>,
) -> AuthoritativeBase {
    match discovery {
        Err(_) => AuthoritativeBase::Static,
        Ok(document) if document.repository_base_url == static_base => AuthoritativeBase::Static,
        Ok(document) => AuthoritativeBase::Discovered(document.repository_base_url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_discovery_document_parses_committed_file() {
        let document: DiscoveryDocument = static_discovery_document().unwrap();

        assert_eq!(document.schema_version, 1);
        assert_eq!(document.repository_base_url, "/repository");
        assert_eq!(document.minimum_client_version, "0.1.0");
        assert_eq!(document.sunset, None);
    }

    #[test]
    fn authoritative_repository_base_uses_static_when_discovery_fails() {
        let result: AuthoritativeBase = authoritative_repository_base(
            "/repository",
            Err(AppError::from("discovery failed".to_string())),
        );

        assert!(matches!(result, AuthoritativeBase::Static));
    }

    #[test]
    fn authoritative_repository_base_uses_static_when_discovery_matches() {
        let document: DiscoveryDocument = DiscoveryDocument {
            schema_version: 1,
            repository_base_url: "/repository".to_string(),
            minimum_client_version: "0.1.0".to_string(),
            sunset: None,
        };

        let result: AuthoritativeBase = authoritative_repository_base("/repository", Ok(document));

        assert!(matches!(result, AuthoritativeBase::Static));
    }

    #[test]
    fn authoritative_repository_base_uses_discovered_when_base_differs() {
        let document: DiscoveryDocument = DiscoveryDocument {
            schema_version: 1,
            repository_base_url: "https://repository.eafora.org".to_string(),
            minimum_client_version: "0.1.0".to_string(),
            sunset: None,
        };

        let result: AuthoritativeBase = authoritative_repository_base("/repository", Ok(document));

        match result {
            AuthoritativeBase::Discovered(url) => {
                assert_eq!(url, "https://repository.eafora.org");
            }
            AuthoritativeBase::Static => panic!(),
        }
    }
}
