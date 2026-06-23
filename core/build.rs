use std::process::Command;

fn main() {
    let revision: String = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|out| if out.status.success() { Some(out.stdout) } else { None })
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=EAFORA_REVISION={}", revision);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
