//! Hosted workflow hygiene must stay locked: dispatch, concurrency,
//! timeouts, harden-runner, persist-credentials, and stealth on the
//! aggregator.

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
        "stealth",
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
    }
}

#[test]
fn stealth_job_is_needed_by_aggregator_ci() {
    let ci = read_rel(".github/workflows/ci.yml");
    let jobs = parse_jobs(&ci);
    let ci_job = jobs
        .iter()
        .find(|(name, _)| name == "ci")
        .unwrap_or_else(|| panic!("ci.yml must have aggregator job id `ci`"));
    assert!(
        ci_job.1.contains("stealth"),
        "aggregator CI needs: must include stealth"
    );
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
