//! Captures compile-time provenance for persistence benchmark reports.

use std::process::Command;

fn main() {
    for name in ["PROFILE", "OPT_LEVEL", "TARGET", "HOST"] {
        if let Ok(value) = std::env::var(name) {
            println!("cargo:rustc-env=NEO_BENCH_BUILD_{name}={value}");
        }
    }

    if let Ok(rustc) = std::env::var("RUSTC") {
        let version = Command::new(rustc)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|version| version.trim().to_owned())
            .unwrap_or_else(|| "unavailable".to_owned());
        println!("cargo:rustc-env=NEO_BENCH_BUILD_RUSTC={version}");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
