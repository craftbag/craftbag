//! Local rustc must match CI. A newer default toolchain can accept
//! let-chains that fail the 1.85 job.

#[test]
fn rust_toolchain_toml_matches_cargo_msrv_and_ci() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml (local rustc follows this, not PATH rustc)");
    assert!(
        toolchain.contains("channel = \"1.85\""),
        "rust-toolchain.toml must pin 1.85: {toolchain}"
    );
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    assert!(
        cargo.contains("rust-version = \"1.85\""),
        "Cargo.toml rust-version must stay 1.85"
    );
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci.yml");
    assert!(
        ci.contains("toolchain: \"1.85\""),
        "ci.yml rust jobs must stay on 1.85"
    );
}
