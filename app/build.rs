use std::{process::Command, time::{SystemTime, UNIX_EPOCH}};
fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
    println!("cargo:rerun-if-changed=src");
    let revision = Command::new("git").args(["describe", "--always", "--dirty"]).output().ok()
        .filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|| "source archive".into());
    let timestamp = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
    println!("cargo:rustc-env=SPIDE_BUILD_REV={revision}");
    println!("cargo:rustc-env=SPIDE_BUILD_EPOCH={timestamp}");
}
