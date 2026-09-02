//! Live probe against Eurostat, ignored by default because it needs network access.
//!
//! Run with: cargo test -p ingestion --test eurostat_live -- --ignored --nocapture

use ingestion::eurostat::eurostat_client::{self, EurostatExtraction};

#[tokio::test]
#[ignore = "requires network access"]
async fn fetch_upstream_returns_every_level_the_adapter_reads() {
    for extraction in &eurostat_client::EXTRACTIONS {
        let body: String = eurostat_client::fetch_upstream(extraction)
            .await
            .expect("eurostat fetch");

        // Written out so the parser and the checked-in samples are built from the real thing.
        let destination: String = destination_of(extraction);
        std::fs::write(&destination, &body).expect("write probe output");

        println!("{} bytes -> {destination}", body.len());
    }
}

fn destination_of(extraction: &EurostatExtraction) -> String {
    format!(
        "/tmp/eurostat-{}-{}.json",
        extraction.dataset,
        extraction.geo_level.code(),
    )
}
