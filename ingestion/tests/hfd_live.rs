//! Live probe against HFD, ignored by default because it needs credentials and the network.
//!
//! Run with: cargo test -p ingestion --test hfd_live -- --ignored --nocapture

use ingestion::hfd::hfd_client;

#[tokio::test]
#[ignore = "requires HFD credentials and network access"]
async fn fetch_upstream_returns_both_fertility_files() {
    let archive: Vec<u8> = hfd_client::fetch_upstream()
        .await
        .expect("hfd fetch");

    println!("archive: {} bytes", archive.len());

    for member_name in [hfd_client::COHORT_MEMBER, hfd_client::PERIOD_MEMBER] {
        let contents: String = hfd_client::read_member(&archive, member_name).expect("read the member");

        println!("\n=== {member_name} ===");
        for line in contents.lines().take(3) {
            println!("{line}");
        }
        // Written out so the parser and the checked-in samples are built from the real thing.
        let destination: String = format!("/tmp/hfd-{member_name}");
        std::fs::write(&destination, &contents).expect("write probe output");
        println!("wrote {destination} ({} bytes)", contents.len());
    }
}
