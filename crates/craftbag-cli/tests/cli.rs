use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;

fn bin() -> (tempfile::TempDir, Command) {
    let home = tempfile::tempdir().expect("home");
    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    cmd.env("HOME", home.path()).env("USERPROFILE", home.path());
    (home, cmd)
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

#[test]
fn validate_minimal_valid_ok() {
    let path = corpus().join("agentskills/minimal-valid/SKILL.md");
    let (_home, mut cmd) = bin();
    cmd.arg("validate")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"));
}

#[test]
fn validate_invalid_name_fails() {
    let path = corpus().join("agentskills/invalid-name/Bad_Name/SKILL.md");
    let (_home, mut cmd) = bin();
    cmd.arg("validate").arg(&path).assert().failure();
}

#[test]
fn list_extra_path_json() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
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
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("load")
        .arg("no-such-skill")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown skill: no-such-skill"),
        "stderr={stderr}"
    );
    assert!(!stderr.contains("skipped skill"), "stderr={stderr}");
}

#[test]
fn load_parse_error_skip_is_not_unknown() {
    let parent = corpus().join("agentskills/invalid-name");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("Bad_Name")
        .arg("--path")
        .arg(&parent)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skipped skill: Bad_Name"),
        "load must name the skipped skill, not call it unknown: {stderr}"
    );
    assert!(
        stderr.contains("parse_error"),
        "load must include skip kind: {stderr}"
    );
    assert!(
        !stderr.contains("unknown skill"),
        "skipped parse error must not look missing: {stderr}"
    );
}

#[test]
fn load_parse_skip_without_frontmatter_name_is_not_unknown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\ndescription: no name\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("demo")
        .arg("--path")
        .arg(tmp.path())
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skipped skill: demo"),
        "load must use the package dir when peek name is missing: {stderr}"
    );
    assert!(
        stderr.contains("parse_error"),
        "load must include skip kind: {stderr}"
    );
    assert!(
        !stderr.contains("unknown skill"),
        "nameless parse skip must not look missing: {stderr}"
    );
}

#[test]
fn why_unknown_exits_1() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("why")
        .arg("no-such-skill")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn why_parse_error_name_is_not_unknown() {
    let parent = corpus().join("agentskills/invalid-name");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
        .arg("Bad_Name")
        .arg("--json")
        .arg("--path")
        .arg(&parent)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"name\": \"Bad_Name\""),
        "why JSON must keep frontmatter name: {stdout}"
    );
    assert!(
        stdout.contains("\"kind\": \"parse_error\""),
        "why JSON must keep parse_error: {stdout}"
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("unknown skill"));
}

#[test]
fn load_minimal_valid() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
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
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("list")
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.to_lowercase().contains("welcome"));
    assert!(!stdout.contains("🚀"));
}
