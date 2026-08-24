use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

fn bin() -> Command {
    Command::cargo_bin("craftbag").expect("bin")
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

#[test]
fn validate_minimal_valid_ok() {
    let path = corpus().join("agentskills/minimal-valid/SKILL.md");
    bin()
        .arg("validate")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"));
}

#[test]
fn validate_invalid_name_fails() {
    let path = corpus().join("agentskills/invalid-name/Bad_Name/SKILL.md");
    bin().arg("validate").arg(&path).assert().failure();
}

#[test]
fn list_extra_path_json() {
    let pkg = corpus().join("agentskills/minimal-valid");
    bin()
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains("minimal-valid"));
}

#[test]
fn load_unknown_exits_2() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = bin()
        .current_dir(tmp.path())
        .arg("load")
        .arg("no-such-skill")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn why_unknown_exits_1() {
    let tmp = tempfile::tempdir().expect("tmp");
    let out = bin()
        .current_dir(tmp.path())
        .arg("why")
        .arg("no-such-skill")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn load_minimal_valid() {
    let pkg = corpus().join("agentskills/minimal-valid");
    bin()
        .arg("load")
        .arg("minimal-valid")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "[Activated skill: minimal-valid]",
        ));
}

#[test]
fn list_does_not_print_banner() {
    let tmp = tempfile::tempdir().expect("tmp");
    fs::create_dir_all(tmp.path().join(".agents").join("skills")).expect("mkdir");
    let out = bin()
        .current_dir(tmp.path())
        .arg("list")
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.to_lowercase().contains("welcome"));
    assert!(!stdout.contains("🚀"));
}
