use std::process::Command;

fn main() {
    let revision: Option<String> = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|out| if out.status.success() { Some(out.stdout) } else { None })
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string());

    let is_shipping_build: bool = std::env::var("PROFILE").unwrap_or_default() != "debug";

    let resolved_revision: String = match revision {
        Some(revision) => revision,
        None if is_shipping_build => panic!(
            "git rev-parse HEAD failed: a release build MUST embed a real source revision \
             (EAFORA_REVISION) for crash symbolication. Build from a full git clone with `git` on PATH.",
        ),
        None => {
            let value: &str = "unknown";
            println!("cargo:warning=git rev-parse HEAD failed; EAFORA_REVISION={value} (debug build)");
            value.to_string()
        }
    };

    println!("cargo:rustc-env=EAFORA_REVISION={}", resolved_revision);
}
