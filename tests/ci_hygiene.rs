//! Hosted workflow hygiene must stay locked: dispatch, concurrency,
//! timeouts, harden-runner, persist-credentials, and aggregator CI.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_rel(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn workflow_files() -> Vec<PathBuf> {
    let dir = repo_root().join(".github/workflows");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|s| s.to_str()), Some("yml" | "yaml")))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "expected workflow YAML under .github/workflows"
    );
    files
}

fn rel_display(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Job id and body (properties + steps) for each `jobs:` entry.
fn parse_jobs(yaml: &str) -> Vec<(String, String)> {
    let mut jobs = Vec::new();
    let mut in_jobs = false;
    let mut current_name: Option<String> = None;
    let mut current_body = String::new();

    for line in yaml.lines() {
        if !in_jobs {
            if line == "jobs:" {
                in_jobs = true;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("  ") {
            if !rest.starts_with(' ') && !rest.starts_with('#') && rest.ends_with(':') {
                if let Some(name) = current_name.take() {
                    jobs.push((name, std::mem::take(&mut current_body)));
                }
                current_name = Some(rest.trim_end_matches(':').to_string());
                continue;
            }
        }
        if current_name.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(name) = current_name {
        jobs.push((name, current_body));
    }
    jobs
}

fn checkout_missing_persist(yaml: &str) -> Option<usize> {
    let lines: Vec<&str> = yaml.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("actions/checkout@") {
            continue;
        }
        let mut found_false = false;
        for next in &lines[i + 1..] {
            let t = next.trim();
            if t.starts_with("- ") {
                break;
            }
            if t.starts_with("persist-credentials:") {
                if t.contains("false") {
                    found_false = true;
                }
                break;
            }
        }
        if !found_false {
            return Some(i + 1);
        }
    }
    None
}

/// Line of a harden-runner step that is still gated to Linux.
///
/// harden-runner v2.15+ runs on Windows and macOS. A leftover
/// `if: runner.os == 'Linux'` skips the step and leaves those jobs
/// unhardened while still passing a string-presence check.
fn harden_runner_linux_gated(yaml: &str) -> Option<usize> {
    let lines: Vec<&str> = yaml.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("step-security/harden-runner@") {
            continue;
        }
        let mut start = i;
        while start > 0 && !lines[start].trim_start().starts_with("- ") {
            start -= 1;
        }
        let indent = lines[start].chars().take_while(|c| *c == ' ').count();
        let mut j = start;
        while j < lines.len() {
            if j > start {
                let next_indent = lines[j].chars().take_while(|c| *c == ' ').count();
                if lines[j].trim_start().starts_with("- ") && next_indent <= indent {
                    break;
                }
            }
            let t = lines[j].trim();
            if t.starts_with("if:") && t.contains("runner.os") && t.contains("Linux") {
                return Some(i + 1);
            }
            j += 1;
        }
    }
    None
}

#[test]
fn parse_jobs_finds_ci_yml_jobs() {
    let ci = read_rel(".github/workflows/ci.yml");
    let names: Vec<String> = parse_jobs(&ci).into_iter().map(|(n, _)| n).collect();
    for expected in [
        "changes",
        "lint",
        "test",
        "fuzz-smoke",
        "test-windows",
        "test-macos",
        "workflows",
        "gitleaks",
        "ci",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing job {expected}: {names:?}"
        );
    }
}

#[test]
fn fuzz_smoke_runs_parse_skill_and_xml_catalog() {
    let ci = read_rel(".github/workflows/ci.yml");
    let fuzz = parse_jobs(&ci)
        .into_iter()
        .find(|(n, _)| n == "fuzz-smoke")
        .map(|(_, body)| body)
        .expect("fuzz-smoke job");
    assert!(
        fuzz.contains("cargo fuzz run parse_skill"),
        "fuzz-smoke must still run parse_skill: {fuzz}"
    );
    assert!(
        fuzz.contains("cargo fuzz run xml_catalog"),
        "fuzz-smoke must run xml_catalog so C0 catalog bytes stay XML 1.0: {fuzz}"
    );
    assert!(
        fuzz.contains("cargo fuzz run list_format"),
        "fuzz-smoke must run list_format so padded and empty tokens stay parsed: {fuzz}"
    );
    assert!(
        fuzz.contains("cargo fuzz run catalog_format"),
        "fuzz-smoke must run catalog_format so list --catalog / MCP catalog items stay one line: {fuzz}"
    );
    assert!(
        fuzz.contains("cargo fuzz run vendor_token"),
        "fuzz-smoke must run vendor_token so --vendor / MCP vendor errors stay one line: {fuzz}"
    );
    assert!(
        fuzz.contains("cargo fuzz run sanitize_error_token"),
        "fuzz-smoke must run sanitize_error_token so echoed error tokens stay one line: {fuzz}"
    );
}

#[test]
fn workflows_have_dispatch_concurrency_timeouts_and_harden() {
    for path in workflow_files() {
        let rel = rel_display(&path);
        let yaml = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        assert!(
            yaml.contains("workflow_dispatch"),
            "{rel} must declare workflow_dispatch"
        );
        assert!(
            yaml.contains("concurrency:"),
            "{rel} must declare a concurrency group"
        );

        let jobs = parse_jobs(&yaml);
        assert!(!jobs.is_empty(), "{rel} must declare jobs");
        for (name, body) in &jobs {
            assert!(
                body.contains("timeout-minutes:"),
                "{rel} job {name} must set timeout-minutes"
            );
            assert!(
                body.contains("step-security/harden-runner@"),
                "{rel} job {name} must use harden-runner"
            );
        }

        if let Some(line) = checkout_missing_persist(&yaml) {
            panic!("{rel} checkout at line {line} must set persist-credentials: false");
        }
        if let Some(line) = harden_runner_linux_gated(&yaml) {
            panic!(
                "{rel} harden-runner at line {line} must not be gated to Linux; v2.15+ runs on Windows and macOS"
            );
        }
    }
}

/// Detect changes can skip the whole rust matrix. If it fails and the
/// aggregator does not check it, lint/test stay skipped and CI is green.
#[test]
fn changes_job_is_checked_by_aggregator_ci() {
    let ci = read_rel(".github/workflows/ci.yml");
    let jobs = parse_jobs(&ci);
    let ci_job = jobs
        .iter()
        .find(|(name, _)| name == "ci")
        .unwrap_or_else(|| panic!("ci.yml must have aggregator job id `ci`"));
    assert!(
        ci_job.1.contains("needs.changes.result"),
        "aggregator must fail when Detect changes fails; otherwise rust jobs skip and CI stays green: {}",
        ci_job.1
    );
}

#[test]
fn rust_path_filter_covers_all_workflows() {
    let ci = read_rel(".github/workflows/ci.yml");
    assert!(
        ci.contains("- '.github/workflows/**'"),
        "rust path-filter must include all workflow files so hygiene tests run"
    );
}

/// Hosted workflow linters must stay on the current patch/minor pins.
/// actionlint 1.7.7 and gitleaks 8.24.3 miss later syntax and leak rules.
#[test]
fn ci_pins_current_workflow_linters() {
    let ci = read_rel(".github/workflows/ci.yml");
    assert!(
        ci.contains("rhysd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca # v1.7.12"),
        "ci.yml must pin actionlint v1.7.12"
    );
    assert!(
        ci.contains("VER=8.30.1"),
        "gitleaks install must pin 8.30.1"
    );
    assert!(
        ci.contains("'zizmor==1.29.0'"),
        "workflows job must pin zizmor 1.29.0"
    );
}

/// taiki-e/install-action `tool: cargo-deny` (no @version) tracks latest.
/// A new cargo-deny or nextest release can change the license graph or
/// CLI flags overnight; local `make check` then disagrees with hosted lint.
#[test]
fn ci_pins_install_action_rust_tools() {
    let ci = read_rel(".github/workflows/ci.yml");
    let mut deny = 0;
    let mut nextest = 0;
    let mut fuzz = 0;
    for line in ci.lines() {
        let t = line.trim();
        if t.starts_with("tool: cargo-deny") {
            assert_eq!(
                t, "tool: cargo-deny@0.20.2",
                "lint must pin cargo-deny 0.20.2 (install-action checksummed manifest)"
            );
            deny += 1;
        }
        if t.starts_with("tool: cargo-nextest") {
            assert_eq!(
                t, "tool: cargo-nextest@0.9.143",
                "test jobs must pin cargo-nextest 0.9.143"
            );
            nextest += 1;
        }
        if t.starts_with("tool: cargo-fuzz") {
            assert_eq!(
                t, "tool: cargo-fuzz@0.13.2",
                "fuzz-smoke must pin cargo-fuzz 0.13.2"
            );
            fuzz += 1;
        }
    }
    assert_eq!(deny, 1, "lint must install cargo-deny once: {deny}");
    assert_eq!(
        nextest, 3,
        "test, test-windows, and test-macos must each pin cargo-nextest: {nextest}"
    );
    assert_eq!(fuzz, 1, "fuzz-smoke must install cargo-fuzz once: {fuzz}");
}

/// Constitution: README names the product. Path-filter must include
/// README.md so the rust matrix runs on a README-only PR.
#[test]
fn readme_names_the_product_and_is_path_filtered() {
    let readme = read_rel("README.md").replace("\r\n", "\n");
    assert!(
        readme.starts_with("# craftbag\n"),
        "README must start with the crate name"
    );
    assert!(
        !readme.contains("Not ready."),
        "README must not stay the stealth placeholder"
    );
    assert!(
        readme.contains("## Getting started"),
        "README must have a Getting started section"
    );
    assert!(
        readme.contains("demo/workspace/.agents/skills"),
        "Getting started must name the demo tree so clone-and-list is not empty"
    );
    assert!(
        readme.contains("## Library"),
        "README must show a library embedder path"
    );
    let ci = read_rel(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'README.md'"),
        "rust path-filter must include README.md so a README-only edit runs this lock"
    );
}

/// Quoted SPDX ids in `deny.toml` `[licenses].allow`.
fn deny_allow_licenses(deny: &str) -> Vec<&str> {
    let after = deny
        .split("allow = [")
        .nth(1)
        .expect("deny.toml must have licenses.allow");
    let body = after
        .split(']')
        .next()
        .expect("deny.toml licenses.allow must close");
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix('"') {
            if let Some(name) = rest.split('"').next() {
                out.push(name);
            }
        }
    }
    out
}

/// Unused allow entries are deny.toml drift. cargo-deny only warns
/// (`license-not-encountered`). A BSD-only crate would then pass.
/// NCSA stays for fuzz `libfuzzer-sys`.
#[test]
fn deny_toml_allow_list_matches_used_licenses() {
    let deny = read_rel("deny.toml");
    assert_eq!(
        deny_allow_licenses(&deny),
        ["Apache-2.0", "MIT", "NCSA", "Unicode-3.0", "Zlib"],
        "deny.toml allow must stay the licenses on the workspace or fuzz graphs"
    );
    let ci = read_rel(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'deny.toml'"),
        "rust path-filter must include deny.toml so an allow-list edit runs this lock"
    );
}

/// `on:` block: from a column-0 `on:` through the next column-0 key.
fn workflow_on_block(yaml: &str) -> &str {
    let start = if yaml.starts_with("on:") {
        0
    } else {
        yaml.find("\non:")
            .map(|i| i + 1)
            .expect("workflow must declare on:")
    };
    let rest = &yaml[start..];
    let after = rest.strip_prefix("on:").expect("on: prefix");
    let end = after
        .lines()
        .skip(1)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with(' ')
                && !line.starts_with('\t')
                && !line.starts_with('#')
        })
        .and_then(|line| after.find(line))
        .unwrap_or(after.len());
    &after[..end]
}

/// One compile per tree: PR (and merge_group) is the matrix.
/// push to main and a tag of that commit must not cargo test again.
/// A later release job may compile once for signing; it must not
/// re-run this matrix (ci-build-once Recipe A / E).
#[test]
fn ci_compiles_once_per_tree() {
    let ci = read_rel(".github/workflows/ci.yml");
    let on = workflow_on_block(&ci);
    assert!(
        on.contains("pull_request:"),
        "ci.yml must run the matrix on pull_request: {on}"
    );
    assert!(
        on.contains("workflow_dispatch:"),
        "ci.yml must keep workflow_dispatch as the escape hatch: {on}"
    );
    assert!(
        !on.contains("branches:"),
        "ci.yml must not compile again on push to main: {on}"
    );
    assert!(
        !on.contains("tags:"),
        "ci.yml must not compile again on a tag of the same commit: {on}"
    );
    for path in workflow_files() {
        let rel = rel_display(&path);
        let yaml = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let on = workflow_on_block(&yaml);
        let release_trigger = on.contains("tags:") || on.contains("release:");
        if !release_trigger {
            continue;
        }
        assert!(
            !yaml.contains("cargo nextest")
                && !yaml.contains("cargo test")
                && !yaml.contains("cargo fuzz")
                && !yaml.contains("cargo clippy")
                && !yaml.contains("cargo build"),
            "{rel} must promote a tested tree on tag/release, not compile the matrix"
        );
    }
}

/// First public crates share one version and are publishable.
#[test]
fn publishable_crates_share_version_and_are_not_hidden() {
    let lib = read_rel("Cargo.toml");
    let cli = read_rel("crates/craftbag-cli/Cargo.toml");
    let mcp = read_rel("crates/craftbag-mcp/Cargo.toml");
    let fuzz = read_rel("fuzz/Cargo.toml");
    let version = lib
        .lines()
        .find(|line| line.starts_with("version = "))
        .expect("lib Cargo.toml must set version");
    assert!(
        cli.contains(version) && mcp.contains(version),
        "cli and mcp versions must match the lib: {version}"
    );
    for (name, body) in [("lib", &lib), ("cli", &cli), ("mcp", &mcp)] {
        assert!(
            !body.contains("publish = false"),
            "{name} must be publishable"
        );
    }
    assert!(
        fuzz.contains("publish = false"),
        "fuzz crate stays unpublished"
    );
    assert!(
        cli.contains("craftbag = { version = ") && mcp.contains("craftbag = { version = "),
        "cli and mcp path deps must pin the published lib version"
    );
    let release = read_rel(".github/workflows/release.yml");
    assert!(
        release.contains("cargo publish") || release.contains("publish-crates.sh"),
        "release.yml must publish crates on a tag"
    );
    assert!(
        release.contains("CARGO_REGISTRY_TOKEN"),
        "release.yml must use CARGO_REGISTRY_TOKEN"
    );
}

/// crates.io must ship compile + license + docs, not factory scripts,
/// brand assets, or demo trees. `include` (or exclude) is the lock;
/// `cargo package --list` is the proof.
#[test]
fn published_lib_crate_omits_factory_demo_brand() {
    let output = std::process::Command::new("cargo")
        .args(["package", "--list", "-p", "craftbag", "--allow-dirty"])
        .current_dir(repo_root())
        .output()
        .expect("cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listed = String::from_utf8_lossy(&output.stdout);
    for forbidden in ["factory/", "demo/", "brand/", "AGENTS.md"] {
        let hit = listed
            .lines()
            .any(|line| line == forbidden || line.starts_with(forbidden));
        assert!(
            !hit,
            "published crate must not include {forbidden}: {listed}"
        );
    }
    for required in [
        "Cargo.toml",
        "LICENSE",
        "LICENSE-APACHE",
        "README.md",
        "CHANGELOG.md",
        "src/lib.rs",
    ] {
        assert!(
            listed.lines().any(|line| line == required),
            "published crate must include {required}: {listed}"
        );
    }
    let lib = read_rel("Cargo.toml");
    assert!(
        lib.contains("\"/README.md\"") && lib.contains("\"/LICENSE-APACHE\""),
        "include must root-anchor README and LICENSE so demo/ and rustup fixtures stay out"
    );
}
