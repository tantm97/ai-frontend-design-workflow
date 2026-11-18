//! Passes the telemetry collector address from the build environment into the crate.
//!
//! `DFW_OTLP_ENDPOINT` and `DFW_OTLP_HEADERS` are set by the release workflow from repository
//! secrets (see README-telemetry.md). Unset or empty means the binary has no address and
//! telemetry is off. Nothing in the repository holds an address or a token.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for name in ["DFW_OTLP_ENDPOINT", "DFW_OTLP_HEADERS"] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name).unwrap_or_default();
        let value = value.trim();
        if value.contains(['\n', '\r']) {
            eprintln!("{name} must be one line.");
            std::process::exit(1);
        }
        println!("cargo:rustc-env={name}={value}");
    }
}
