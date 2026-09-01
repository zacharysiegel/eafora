//! Live probe against Eurostat, ignored by default because it needs network access.
//!
//! Run with: cargo test -p ingestion --test eurostat_live -- --ignored --nocapture

use ingestion::eurostat::eurostat_client;

#[tokio::test]
#[ignore = "requires network access"]
async fn fetch_upstream_returns_the_country_level_extraction() {
    let body: String = eurostat_client::fetch_upstream()
        .await
        .expect("eurostat fetch");

    println!("body: {} bytes", body.len());

    // Written out so the parser and the checked-in samples are built from the real thing.
    let destination: &str = "/tmp/eurostat-demo-find-country.json";
    std::fs::write(destination, &body).expect("write probe output");
    println!("wrote {destination}");
}
