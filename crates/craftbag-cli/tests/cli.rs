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
fn validate_unknown_key_passes_default_and_fails_strict() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: d\nmade_up_field: x\n---\nbody\n",
    )
    .expect("write");
    let skill = pkg.join("SKILL.md");
    let (_home, mut cmd) = bin();
    cmd.arg("validate")
        .arg(&skill)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("validate")
        .arg("--strict")
        .arg(&skill)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "strict must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("made_up_field"),
        "strict must name the unknown key: {stderr}"
    );
}

#[test]
fn validate_strict_corpus_ok() {
    let path = corpus().join("agentskills/minimal-valid/SKILL.md");
    let (_home, mut cmd) = bin();
    cmd.arg("validate")
        .arg("--strict")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"));
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
fn list_user_dir_expands_tilde() {
    let (home, mut cmd) = bin();
    let pkg = home.path().join("myskills").join("mine");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: mine\ndescription: from-home\n---\nfrom-home\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    cmd.current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--user-dir")
        .arg("~/myskills")
        .assert()
        .success()
        .stdout(predicates::str::contains("mine"));
}

#[test]
fn list_empty_user_dir_is_rejected() {
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--user-dir")
        .arg("")
        .output()
        .expect("run");
    assert_ne!(
        out.status.code(),
        Some(0),
        "empty --user-dir must not list cwd: stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("user-dir") || stderr.contains("required"),
        "CLI must reject empty --user-dir: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("\"skills\""),
        "empty --user-dir must not print a catalog"
    );
}

#[test]
fn list_user_dir_relative_joins_cwd() {
    let cwd = tempfile::tempdir().expect("cwd");
    let pkg = cwd.path().join("myskills").join("mine");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: mine\ndescription: from-rel\n---\nfrom-rel\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--user-dir")
        .arg("myskills")
        .assert()
        .success()
        .stdout(predicates::str::contains("mine"));
}

#[test]
fn list_xml_available_skills() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--xml")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains("<available_skills>"))
        .stdout(predicates::str::contains("<name>minimal-valid</name>"));
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
fn why_parse_skip_without_frontmatter_name_is_not_unknown() {
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
        .arg("why")
        .arg("demo")
        .arg("--json")
        .arg("--path")
        .arg(tmp.path())
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
        stdout.contains("\"kind\": \"parse_error\""),
        "why JSON must keep parse_error: {stdout}"
    );
    assert!(
        stdout.contains("\"code\": \"parse_error\""),
        "why JSON must include machine code: {stdout}"
    );
    assert!(
        stdout.contains("demo"),
        "why JSON must keep the package path: {stdout}"
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("unknown skill"));
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
fn load_named_root_file_skip_is_not_unknown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let skills = tmp.path().join(".agents").join("skills");
    fs::create_dir_all(&skills).expect("mkdir");
    fs::write(
        skills.join("SKILL.md"),
        "---\nname: loose\ndescription: loose\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("load")
        .arg("loose")
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
        stderr.contains("skipped skill: loose"),
        "load must name the root-file skip, not call it unknown: {stderr}"
    );
    assert!(stderr.contains("root_file"), "stderr={stderr}");
    assert!(
        !stderr.contains("unknown skill"),
        "named root-file skip must not look missing: {stderr}"
    );
}

#[test]
fn why_named_root_file_skip_is_not_unknown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let skills = tmp.path().join(".agents").join("skills");
    fs::create_dir_all(&skills).expect("mkdir");
    fs::write(
        skills.join("SKILL.md"),
        "---\nname: loose\ndescription: loose\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("why")
        .arg("loose")
        .arg("--json")
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
        stdout.contains("\"name\": \"loose\""),
        "why JSON must keep frontmatter name: {stdout}"
    );
    assert!(
        stdout.contains("\"kind\": \"root_file\""),
        "why JSON must keep root_file: {stdout}"
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("unknown skill"));
}

#[test]
fn load_extra_path_root_file_does_not_hide_package() {
    let extra = tempfile::tempdir().expect("extra");
    fs::write(
        extra.path().join("SKILL.md"),
        "---\nname: demo\ndescription: loose\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: package\n---\npackage body\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("demo")
        .arg("--path")
        .arg(extra.path())
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
        stdout.contains("[Activated skill: demo]"),
        "package must load: {stdout}"
    );
    assert!(
        stdout.contains("package body"),
        "must load the named package, not the loose file: {stdout}"
    );
}

#[test]
fn why_extra_path_root_file_and_package_agree_with_load() {
    let extra = tempfile::tempdir().expect("extra");
    fs::write(
        extra.path().join("SKILL.md"),
        "---\nname: demo\ndescription: loose\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: package\n---\npackage body\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
        .arg("demo")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let why_v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    assert_eq!(
        why_v["loaded"][0]["name"], "demo",
        "why must list the loaded package: {stdout}"
    );
    assert_eq!(
        why_v["skips"][0]["kind"], "root_file",
        "why must keep the loose-file skip: {stdout}"
    );
    assert_eq!(why_v["skips"][0]["name"], "demo", "{stdout}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("unknown skill"));
}

#[test]
fn load_extra_path_root_skill_md_does_not_hide_skills_subdir() {
    let extra = tempfile::tempdir().expect("extra");
    fs::write(
        extra.path().join("SKILL.md"),
        "---\nname: loose\ndescription: leftover\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.path().join("skills").join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: from-skills\n---\nfrom-skills\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("public")
        .arg("--path")
        .arg(extra.path())
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
        stdout.contains("[Activated skill: public]"),
        "skills/public must load: {stdout}"
    );
    assert!(
        stdout.contains("from-skills"),
        "must load the skills/ package, not the leftover root file: {stdout}"
    );
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
