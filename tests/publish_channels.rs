//! Publish-channel files stay executable: Chocolatey checksums, the MCPB
//! launcher, and the release jobs that call those workflows.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn chocolatey_package_self_test() {
    let output = Command::new("python3")
        .arg(root().join("scripts/update-chocolatey-package.py"))
        .arg("--self-test")
        .current_dir(root())
        .output()
        .expect("python3");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "chocolatey self-test failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("DONE: ok=true"),
        "self-test must print DONE: {stdout}"
    );
}

#[test]
fn chocolatey_v020_matches_the_windows_zip() {
    let script = std::fs::read(root().join("chocolatey/tools/chocolateyInstall.ps1"))
        .expect("install script");
    assert!(
        script.starts_with(b"\xef\xbb\xbf"),
        "chocolateyInstall.ps1 must be UTF-8 with BOM"
    );
    let text = String::from_utf8_lossy(&script);
    assert!(text.contains(
        "https://github.com/craftbag/craftbag/releases/download/v0.2.0/craftbag-x86_64-pc-windows-msvc.zip"
    ));
    assert!(text.contains("e3fa8856e021b17551f9dada72c81e0dc99f6b70258b4577562f25c259fc5f3f"));
    assert!(!text.contains("urlArm64"));
    let nuspec = read("chocolatey/craftbag.nuspec");
    assert!(nuspec.contains("<id>craftbag</id>"));
    assert!(nuspec.contains("<version>0.2.0</version>"));
    assert!(nuspec.contains("<projectUrl>https://docs.rs/craftbag</projectUrl>"));
    assert!(
        nuspec
            .contains("<projectSourceUrl>https://github.com/craftbag/craftbag</projectSourceUrl>")
    );
}

#[test]
fn mcpb_launcher_uses_the_path_binary() {
    let launcher = read("mcpb/server/run.mjs");
    assert!(launcher.contains("craftbag-mcp"));
    assert!(
        !launcher.contains("npx"),
        "craftbag has no npm package; the launcher must not call npx"
    );
    let manifest = read("mcpb/manifest.json");
    assert!(manifest.contains("\"command\": \"craftbag-mcp\""));
}

#[test]
#[cfg(unix)]
fn pack_mcpb_self_test() {
    let output = Command::new("bash")
        .arg(root().join("scripts/pack-mcpb.sh"))
        .arg("--self-test")
        .current_dir(root())
        .output()
        .expect("bash");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pack-mcpb self-test failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("DONE: ok=true"), "{stdout}");
}

#[test]
fn release_calls_publish_channels_after_assets() {
    let release = read(".github/workflows/release.yml");
    for workflow in [
        "publish-winget.yml",
        "publish-chocolatey.yml",
        "publish-smithery.yml",
        "publish-mcp-registry.yml",
    ] {
        assert!(
            release.contains(&format!("uses: ./.github/workflows/{workflow}")),
            "release.yml must call {workflow}"
        );
    }
    let winget = read(".github/workflows/publish-winget.yml");
    assert!(winget.contains("identifier: Craftbag.Craftbag"));
    assert!(winget.contains("fork-user: SebTardif"));
    let chocolatey = read(".github/workflows/publish-chocolatey.yml");
    assert!(chocolatey.contains("choco pack"));
    assert!(chocolatey.contains("push.chocolatey.org"));
    assert!(
        !chocolatey.contains("Add chocolatey/craftbag.nuspec"),
        "chocolatey workflow must pack instead of failing when the key is set"
    );
    let smithery = read(".github/workflows/publish-smithery.yml");
    assert!(smithery.contains("scripts/pack-mcpb.sh"));
    assert!(smithery.contains("scripts/publish-smithery.sh"));
    let registry = read(".github/workflows/publish-mcp-registry.yml");
    assert!(registry.contains("workflow_call"));
    assert!(registry.contains("mcp-name:"));
    assert!(registry.contains("description is"));
}

#[test]
fn registry_marker_is_visible_readme_text() {
    let readme = read("crates/craftbag-mcp/README.md");
    assert!(readme.contains("mcp-name: io.github.craftbag/craftbag-mcp"));
    assert!(
        !readme.contains("<!--"),
        "cargo ownership markers cannot be HTML comments"
    );
    let server = read("server.json");
    let value: serde_json::Value = serde_json::from_str(&server).expect("server.json");
    assert_eq!(
        value["name"].as_str(),
        Some("io.github.craftbag/craftbag-mcp")
    );
    let description = value["description"].as_str().expect("description");
    assert!(
        description.chars().count() <= 100,
        "description is {} chars",
        description.chars().count()
    );
}

#[test]
fn ci_path_filter_covers_publish_channels() {
    let ci = read(".github/workflows/ci.yml");
    for line in ["scripts/**", "chocolatey/**", "mcpb/**", "server.json"] {
        assert!(
            ci.contains(&format!("- '{line}'")),
            "rust path-filter must include {line}"
        );
    }
}
