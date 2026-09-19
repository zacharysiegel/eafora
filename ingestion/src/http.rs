use std::sync::LazyLock;

/// `reqwest::Client` pools connections, caches DNS, and holds TLS configuration internally.
pub static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("eafora/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP_CLIENT build")
});
