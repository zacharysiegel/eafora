use std::sync::LazyLock;

/// Process-wide HTTP client. `reqwest::Client` already pools connections,
/// caches DNS, and holds TLS config; sharing one instance keeps that state
/// alive across calls instead of building it fresh per request.
pub static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("eafora/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP_CLIENT build")
});
