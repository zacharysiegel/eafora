//! Live probe against HFD, ignored by default because it needs credentials and the network.
//!
//! Run with: cargo test -p ingestion --test hfd_live -- --ignored --nocapture

use ingestion::hfd::hfd_client;
use ingestion::hfd::hfd_client::CohortFertilityFile;

#[tokio::test]
#[ignore = "requires HFD credentials and network access"]
async fn fetch_upstream_returns_cohort_files() {
    let files: Vec<CohortFertilityFile> = hfd_client::fetch_upstream()
        .await
        .expect("hfd fetch");

    println!("cohort members: {}", files.len());

    for file in files.iter() {
        println!("\n=== {} ===", file.member_name);
        for line in file.contents.lines().take(3) {
            println!("{line}");
        }
        // Written out so the parser and the checked-in samples are built from the real thing.
        let destination: String = format!("/tmp/hfd-{}", file.member_name);
        std::fs::write(&destination, &file.contents).expect("write probe output");
        println!("wrote {destination} ({} bytes)", file.contents.len());
    }

    assert!(!files.is_empty());
}
