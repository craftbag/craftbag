//! Homebrew / Scoop / combined-archive generators stay executable without
//! a Rust toolchain on the user's machine.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_file(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|e| panic!("read {rel}: {e}"))
        .replace("\r\n", "\n")
}

#[test]
fn publish_install_channels_self_test() {
    let script = repo_root().join("factory/scripts/publish-install-channels.py");
    let output = Command::new("python3")
        .arg(&script)
        .arg("--self-test")
        .current_dir(repo_root())
        .output()
        .expect("python3 factory/scripts/publish-install-channels.py --self-test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "install-channel self-test failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("DONE: ok=true"),
        "self-test must print DONE: {stdout}"
    );
}

#[test]
fn release_tag_workflow_uses_dist_build_not_cargo_build() {
    let release = repo_file(".github/workflows/release.yml");
    assert!(
        release.contains("dist build --artifacts=local"),
        "release.yml must compile archives with dist build"
    );
    assert!(
        release.contains("publish-install-channels.py"),
        "release.yml must pack combined craftbag + craftbag-mcp archives"
    );
    assert!(
        release.contains("craftbag/homebrew-tap") && release.contains("craftbag/scoop-bucket"),
        "release.yml must push Homebrew and Scoop metadata"
    );
    assert!(
        !release.contains("cargo build") && !release.contains("cargo test"),
        "release.yml must not contain cargo build/test (compile-once lock)"
    );
}

#[test]
fn rust_path_filter_covers_install_channel_sources() {
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'factory/scripts/**'"),
        "rust path-filter must include factory/scripts so packer tests run"
    );
    assert!(
        ci.contains("- 'dist-workspace.toml'"),
        "rust path-filter must include dist-workspace.toml"
    );
}

#[test]
fn cargo_toml_defines_dist_profile() {
    let cargo = repo_file("Cargo.toml");
    assert!(
        cargo.contains("[profile.dist]"),
        "dist build uses --profile=dist; Cargo.toml must define it"
    );
    assert!(
        cargo.contains("inherits = \"release\""),
        "profile.dist must inherit release"
    );
}

#[test]
fn dist_workspace_keeps_hand_written_release_ci() {
    let dist = repo_file("dist-workspace.toml");
    assert!(
        dist.contains("allow-dirty = [\"ci\"]"),
        "dist generate must not overwrite release.yml: {dist}"
    );
    assert!(
        dist.contains("installers = []"),
        "combined archives are packed by factory scripts, not cargo-dist installers"
    );
}
