use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

fn stdout_has_path(stdout: &str, want: &Path) -> bool {
    stdout.lines().any(|l| Path::new(l) == want)
}

fn list_json_source(stdout: &str, name: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let row = skills
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("missing skill {name}: {stdout}"));
    row["source"]
        .as_str()
        .unwrap_or_else(|| panic!("source must be a string, not {row}: {stdout}"))
        .to_owned()
}

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
fn validate_json_exposes_error_kind() {
    let path = corpus().join("agentskills/invalid-name/Bad_Name/SKILL.md");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("validate")
        .arg("--json")
        .arg(&path)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "invalid name must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "validate --json must keep the human one-line: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("validate json");
    assert_eq!(v["error_kind"], "parse_error", "stdout={stdout}");
    assert_eq!(v["error"], stderr.trim(), "stdout={stdout} stderr={stderr}");
    assert_eq!(
        v["path"].as_str().map(std::path::Path::new),
        Some(path.as_path()),
        "validate --json must keep the SKILL.md path: {stdout}"
    );
    assert!(
        v.get("errorKind").is_none(),
        "error_kind must stay snake_case: {stdout}"
    );
}

#[test]
fn validate_json_name_mismatch_exposes_error_kind() {
    let path = corpus().join("agentskills/name-mismatch/wrong-dir/SKILL.md");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("validate")
        .arg("--json")
        .arg(&path)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "name mismatch must fail");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("validate json");
    assert_eq!(
        v["error_kind"], "name_directory_mismatch",
        "stdout={stdout}"
    );
    assert!(
        v["error"].as_str().is_some_and(|e| e.contains("good-name")),
        "stdout={stdout}"
    );
    assert_eq!(
        v["path"].as_str().map(std::path::Path::new),
        Some(path.as_path()),
        "validate --json must keep the SKILL.md path: {stdout}"
    );
}

#[test]
fn validate_json_ok_has_no_error_kind() {
    let path = corpus().join("agentskills/minimal-valid/SKILL.md");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("validate")
        .arg("--json")
        .arg(&path)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("validate json");
    assert_eq!(v["ok"], true, "stdout={stdout}");
    assert_eq!(v["name"], "minimal-valid", "stdout={stdout}");
    assert!(
        v.get("error_kind").is_none(),
        "ok validate must not peel a miss: {stdout}"
    );
}

#[test]
fn validate_json_hostile_unknown_key_stays_one_line() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: d\nevil\u{2028}key: x\n---\nbody\n",
    )
    .expect("write");
    let skill = pkg.join("SKILL.md");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("validate")
        .arg("--strict")
        .arg("--json")
        .arg(&skill)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "strict hostile key must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "validate --json must keep one stderr line: {stderr:?}"
    );
    assert!(
        !stderr.contains('\u{2028}'),
        "U+2028 must not leak into stderr: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("validate json");
    assert_eq!(v["error_kind"], "parse_error", "stdout={stdout}");
    assert_eq!(v["error"], stderr.trim(), "stdout={stdout} stderr={stderr}");
    assert_eq!(
        v["path"].as_str().map(std::path::Path::new),
        Some(skill.as_path()),
        "validate --json must keep the SKILL.md path: {stdout}"
    );
    assert!(
        v["error"]
            .as_str()
            .is_some_and(|e| e.contains("evil?key") && !e.contains('\u{2028}')),
        "stdout={stdout}"
    );
}

#[test]
fn list_extra_path_json() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(&pkg)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "minimal-valid"),
        "extra",
        "list JSON source must be the wire token extra, not extraPath: {stdout}"
    );
}

#[test]
fn list_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the CLI door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus().join("leftover/empty-nested-skills");
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "wanted"),
        "extra",
        "empty extra/skills must not hide leftover sibling wanted: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["wanted"],
        "empty extra/skills must not hide leftover sibling packages: {stdout}"
    );
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "empty extra/skills is not a skip: {stdout}"
    );
}

#[test]
fn list_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the CLI door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus().join("leftover/skills-named-package");
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "wanted"),
        "extra",
        "named extra/skills must not hide leftover sibling wanted: {stdout}"
    );
    assert_eq!(
        list_json_source(&stdout, "skills"),
        "extra",
        "named extra/skills package must load: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let mut names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["skills", "wanted"],
        "named extra/skills must not hide leftover sibling or scan nested evil: {stdout}"
    );
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "named extra/skills is not a leftover root file: {stdout}"
    );
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
fn list_empty_path_does_not_scan_cwd() {
    let cwd = tempfile::tempdir().expect("cwd");
    let pkg = cwd.path().join("planted");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: planted\ndescription: from-cwd\n---\nFROM_CWD\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg("")
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() || stderr.contains("path") || stderr.contains("required"),
        "empty --path must not crash: status={:?} stderr={stderr}",
        out.status.code()
    );
    assert!(
        !stdout.contains("planted") && !stdout.contains("FROM_CWD"),
        "empty --path must not load cwd package: {stdout}"
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
        .stdout(predicates::str::contains("<name>minimal-valid</name>"))
        .stdout(predicates::str::contains("<source>extra</source>"));
}

#[test]
fn list_catalog_prints_markdown_names() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--catalog")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains("## Skills"))
        .stdout(predicates::str::contains("minimal-valid"))
        .stdout(predicates::str::contains("Use the host activate command"));
}

#[test]
fn list_watch_dirs_lists_extra_collection() {
    let extra = corpus().join("incumbent/vercel-npx");
    let extra_skills = extra.join("skills");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(&extra)
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
        stdout_has_path(&stdout, &extra),
        "must watch the extra-path collection root: {stdout}"
    );
    assert!(
        stdout_has_path(&stdout, &extra_skills),
        "must watch extra/skills when discover walks it: {stdout}"
    );
    assert!(
        !stdout.contains("deploy-hint") && !stdout.contains("## Skills"),
        "watch-dirs must not load SKILL.md: {stdout}"
    );
}

#[test]
fn list_watch_dirs_lists_extra_path_skill_md_file() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("wanted");
    fs::create_dir_all(&pkg).expect("mkdir");
    let skill = pkg.join("SKILL.md");
    fs::write(&skill, "---\nname: wanted\ndescription: d\n---\nbody\n").expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(&skill)
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
        stdout_has_path(&stdout, &skill),
        "must watch an extra-path SKILL.md file: {stdout}"
    );
    assert!(
        !stdout.contains("## Skills") && !stdout.contains("description: d"),
        "watch-dirs must not load SKILL.md: {stdout}"
    );
}

#[test]
fn list_watch_dirs_omits_named_package_skills_subdir() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(&pkg)
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
        stdout_has_path(&stdout, &pkg),
        "must watch the named extra-path package: {stdout}"
    );
    assert!(
        !stdout
            .lines()
            .any(|l| l.ends_with("minimal-valid/skills") || l.ends_with("minimal-valid\\skills")),
        "named extra-path package must not watch nested skills/: {stdout}"
    );
}

#[test]
fn list_watch_dirs_vendor_claude_lists_user_home() {
    let home = corpus().join("incumbent/claude-user");
    let want = home.join(".claude").join("skills");
    let cwd = tempfile::tempdir().expect("cwd");
    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let out = cmd
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .current_dir(cwd.path())
        .arg("list")
        .arg("--watch-dirs")
        .arg("--vendor")
        .arg("claude")
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
        stdout_has_path(&stdout, &want),
        "must watch HOME/.claude/skills when vendor claude is on: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn list_watch_dirs_omits_escaped_project_skills_root() {
    let cwd = tempfile::tempdir().expect("cwd");
    let outside = tempfile::tempdir().expect("out");
    fs::create_dir_all(outside.path().join("stolen")).expect("mkdir");
    fs::write(
        outside.path().join("stolen").join("SKILL.md"),
        "---\nname: stolen\ndescription: leaked\n---\nSECRET_BODY\n",
    )
    .expect("write");
    let agents = cwd.path().join(".agents");
    fs::create_dir_all(&agents).expect("mkdir .agents");
    let agents_skills = agents.join("skills");
    std::os::unix::fs::symlink(outside.path(), &agents_skills).expect("symlink");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--watch-dirs")
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
        !stdout_has_path(&stdout, &agents_skills),
        "must not watch escaped .agents/skills: {stdout}"
    );
    assert!(
        !stdout_has_path(&stdout, outside.path()),
        "must not list the escaped target: {stdout}"
    );
}

#[test]
fn list_watch_dirs_conflicts_with_json() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--watch-dirs")
        .arg("--json")
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn list_format_json_matches_json_flag() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("json")
        .arg("--path")
        .arg(&pkg)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "minimal-valid"),
        "extra",
        "--format json source must be extra, not extraPath: {stdout}"
    );
}

#[test]
fn list_format_padded_json_matches_json() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg(" json ")
        .arg("--path")
        .arg(&pkg)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "minimal-valid"),
        "extra",
        "--format with spaces around json must still emit JSON: {stdout}"
    );
}

#[test]
fn list_format_xml_matches_xml_flag() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--format")
        .arg("xml")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains("<available_skills>"))
        .stdout(predicates::str::contains("<name>minimal-valid</name>"))
        .stdout(predicates::str::contains("<source>extra</source>"));
}

#[test]
fn list_format_catalog_matches_catalog_flag() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--format")
        .arg("catalog")
        .arg("--path")
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicates::str::contains("## Skills"))
        .stdout(predicates::str::contains("minimal-valid"))
        .stdout(predicates::str::contains("Use the host activate command"));
}

#[test]
fn list_format_watch_lists_extra_collection() {
    let extra = corpus().join("incumbent/vercel-npx");
    let extra_skills = extra.join("skills");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("watch")
        .arg("--path")
        .arg(&extra)
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
        stdout_has_path(&stdout, &extra),
        "--format watch must list the extra-path collection root: {stdout}"
    );
    assert!(
        stdout_has_path(&stdout, &extra_skills),
        "--format watch must list extra/skills: {stdout}"
    );
    assert!(
        !stdout.contains("deploy-hint") && !stdout.contains("## Skills"),
        "--format watch must not load SKILL.md: {stdout}"
    );
}

#[test]
fn list_format_watch_dirs_matches_watch() {
    let extra = corpus().join("incumbent/vercel-npx");
    let extra_skills = extra.join("skills");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("watch-dirs")
        .arg("--path")
        .arg(&extra)
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
        stdout_has_path(&stdout, &extra),
        "--format watch-dirs must list the extra-path collection root: {stdout}"
    );
    assert!(
        stdout_has_path(&stdout, &extra_skills),
        "--format watch-dirs must list extra/skills: {stdout}"
    );
    assert!(
        !stdout.contains("deploy-hint") && !stdout.contains("## Skills"),
        "--format watch-dirs must not load SKILL.md: {stdout}"
    );
}

#[test]
fn list_format_uppercase_suggests_lowercase() {
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("JSON")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "uppercase format must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown format: JSON") && stderr.contains("did you mean json?"),
        "must point at the lowercase token: {stderr}"
    );
}

#[test]
fn list_format_empty_names_valid_tokens() {
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("   ")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "empty format must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown format: empty"),
        "must name the empty token: {stderr}"
    );
    assert!(
        stderr.contains("json")
            && stderr.contains("xml")
            && stderr.contains("catalog")
            && stderr.contains("watch"),
        "must name MCP-matching tokens: {stderr}"
    );
}

#[test]
fn list_format_unknown_names_valid_tokens() {
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("yaml")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "unknown format must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown format: yaml"),
        "must name the bad token: {stderr}"
    );
    assert!(
        stderr.contains("json")
            && stderr.contains("xml")
            && stderr.contains("catalog")
            && stderr.contains("watch"),
        "must name MCP-matching tokens: {stderr}"
    );
}

#[test]
fn list_format_newline_stays_one_stderr_line() {
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--format")
        .arg("json\nxml")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1), "newline format must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown format: json?xml"),
        "control chars must not split stderr: {stderr}"
    );
    assert!(
        !stderr.contains("unknown format: json\nxml"),
        "must not echo a raw newline: {stderr}"
    );
}

#[test]
fn list_format_conflicts_with_json() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--format")
        .arg("json")
        .arg("--json")
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn list_xml_includes_invocation_flags() {
    let extra = tempfile::tempdir().expect("extra");
    let hidden = extra.path().join("hidden-slash");
    fs::create_dir_all(&hidden).expect("mkdir");
    fs::write(
        hidden.join("SKILL.md"),
        "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\ndisable_model_invocation: false\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--xml")
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
    assert!(stdout.contains("<name>hidden-slash</name>"), "xml={stdout}");
    assert!(
        stdout.contains("<user_invocable>false</user_invocable>"),
        "list XML must carry user_invocable for slash palettes: {stdout}"
    );
    assert!(
        stdout.contains("<disable_model_invocation>false</disable_model_invocation>"),
        "list XML must carry disable_model_invocation: {stdout}"
    );
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
fn load_unknown_json_exposes_error_kind() {
    // CLI load is the last miss surface that only printed Display.
    // why --json / validate --json already peel SkillMiss.
    let tmp = tempfile::tempdir().expect("tmp");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("load")
        .arg("no-such-skill")
        .arg("--json")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.trim(),
        "unknown skill: no-such-skill",
        "load --json must keep the human one-line: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("load json");
    assert_eq!(v["error_kind"], "unknown_skill", "stdout={stdout}");
    assert_eq!(
        v["error"], "unknown skill: no-such-skill",
        "stdout={stdout}"
    );
    assert!(
        v.get("errorKind").is_none(),
        "error_kind must stay snake_case: {stdout}"
    );
    let mut keys: Vec<_> = v.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["error".to_owned(), "error_kind".to_owned()],
        "load --json unknown peel is {{ error_kind, error }}; path is omitted: {stdout}"
    );
    assert!(
        v.get("winner_path").is_none() && v.get("winnerPath").is_none(),
        "load --json unknown must omit winner_path like why --json: {stdout}"
    );
}

#[test]
fn load_parse_error_json_exposes_error_kind() {
    let parent = corpus().join("agentskills/invalid-name");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("Bad_Name")
        .arg("--json")
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
        "load --json must keep the human one-line: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("load json");
    assert_eq!(v["error_kind"], "parse_error", "stdout={stdout}");
    assert_eq!(v["error"], stderr.trim(), "stdout={stdout} stderr={stderr}");
    let peeled = v["path"].as_str().expect("path");
    assert!(
        peeled.ends_with("Bad_Name/SKILL.md") || peeled.ends_with("Bad_Name\\SKILL.md"),
        "load --json skip must peel path, not scrape at : {stdout}"
    );
    assert!(
        v.get("winner_path").is_none() && v.get("winnerPath").is_none(),
        "load --json parse_error must omit winner_path: {stdout}"
    );
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    assert_eq!(
        v["skips"][0]["kind"], "parse_error",
        "why JSON must keep parse_error: {stdout}"
    );
    assert_eq!(
        v["skips"][0]["code"], "parse_error",
        "why JSON must include machine code: {stdout}"
    );
    let skip_path = v["skips"][0]["path"].as_str().expect("skip path");
    assert!(
        skip_path.ends_with("demo/SKILL.md") || skip_path.ends_with("demo\\SKILL.md"),
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown skill: no-such-skill"),
        "why must name the miss like load: {stderr}"
    );
    assert_eq!(
        stderr
            .lines()
            .filter(|l| l.contains("unknown skill"))
            .count(),
        1,
        "why unknown must stay one stderr line: {stderr:?}"
    );
}

#[test]
fn why_unknown_json_exposes_error_kind() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("why")
        .arg("no-such-skill")
        .arg("--json")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.trim(),
        "unknown skill: no-such-skill",
        "why --json must keep the human one-line: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    assert_eq!(v["error_kind"], "unknown_skill", "stdout={stdout}");
    assert_eq!(
        v["error"], "unknown skill: no-such-skill",
        "stdout={stdout}"
    );
    assert!(
        v.get("errorKind").is_none(),
        "error_kind must stay snake_case: {stdout}"
    );
    let mut keys: Vec<_> = v.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["error".to_owned(), "error_kind".to_owned()],
        "why --json unknown peel is {{ error_kind, error }}; path is omitted: {stdout}"
    );
    assert!(
        v.get("winner_path").is_none() && v.get("winnerPath").is_none(),
        "why --json unknown must omit winner_path like MCP skills_why: {stdout}"
    );
}

#[test]
fn why_load_validate_collision_winner_path() {
    // Discover already records SkillSkip.winner_path. CLI why/load/validate
    // must expose that on the process wire (MCP merge_skill_miss is the
    // sibling lock). why --json of the collided name is a WhyReport, not a
    // SkillMiss peel, because the winner stays loaded.
    let tmp = tempfile::tempdir().expect("tmp");
    let a = corpus().join("collision/a");
    let b = corpus().join("collision/b");
    let loser = b.join("foo").join("SKILL.md");

    let (_home, mut why) = bin();
    let why_out = why
        .current_dir(tmp.path())
        .arg("why")
        .arg("foo")
        .arg("--json")
        .arg("--no-implicit-roots")
        .arg("--path")
        .arg(&a)
        .arg("--path")
        .arg(&b)
        .output()
        .expect("run");
    assert_eq!(
        why_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&why_out.stderr)
    );
    let why_stdout = String::from_utf8_lossy(&why_out.stdout);
    let why_v: serde_json::Value = serde_json::from_str(&why_stdout).expect("why json");
    assert_eq!(why_v["loaded"][0]["name"], "foo", "{why_stdout}");
    assert_eq!(why_v["skips"][0]["kind"], "name_collision", "{why_stdout}");
    assert_eq!(why_v["skips"][0]["code"], "name_collision", "{why_stdout}");
    let skip_path = why_v["skips"][0]["path"].as_str().expect("skip path");
    assert!(
        skip_path.ends_with("collision/b/foo/SKILL.md")
            || skip_path.ends_with("collision\\b\\foo\\SKILL.md"),
        "why skip path is the loser SKILL.md: {why_stdout}"
    );
    let skip_winner = why_v["skips"][0]["winnerPath"]
        .as_str()
        .expect("winnerPath");
    assert!(
        skip_winner.ends_with("collision/a/foo/SKILL.md")
            || skip_winner.ends_with("collision\\a\\foo\\SKILL.md"),
        "why --json skip must emit camelCase winnerPath, not scrape lost to: {why_stdout}"
    );
    assert!(
        why_v["skips"][0].get("winner_path").is_none(),
        "SkillSkip winnerPath stays camelCase; SkillMiss peel is snake_case: {why_stdout}"
    );
    assert!(
        why_v.get("winner_path").is_none() && why_v.get("error_kind").is_none(),
        "named collision is a WhyReport, not a SkillMiss peel: {why_stdout}"
    );

    let (_home, mut load) = bin();
    let load_out = load
        .current_dir(tmp.path())
        .arg("load")
        .arg("foo")
        .arg("--no-implicit-roots")
        .arg("--path")
        .arg(&a)
        .arg("--path")
        .arg(&b)
        .output()
        .expect("run");
    assert_eq!(
        load_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&load_out.stderr)
    );
    let load_stdout = String::from_utf8_lossy(&load_out.stdout);
    assert!(
        load_stdout.contains("[Activated skill: foo]"),
        "load of a collided name must return the winner: {load_stdout}"
    );
    assert!(
        load_stdout.contains("First path wins"),
        "load must print the winner body, not the loser: {load_stdout}"
    );
    assert!(
        !load_stdout.contains("Second path is skipped"),
        "load must not print the loser body: {load_stdout}"
    );

    let (_home, mut validate_loser) = bin();
    let loser_out = validate_loser
        .arg("validate")
        .arg("--json")
        .arg(&loser)
        .output()
        .expect("run");
    assert_eq!(
        loser_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&loser_out.stderr)
    );
    let loser_stdout = String::from_utf8_lossy(&loser_out.stdout);
    let loser_v: serde_json::Value = serde_json::from_str(&loser_stdout).expect("validate json");
    assert_eq!(loser_v["ok"], true, "{loser_stdout}");
    assert!(
        loser_v.get("error_kind").is_none()
            && loser_v.get("winner_path").is_none()
            && loser_v.get("winnerPath").is_none(),
        "validate is one path, so a collided SKILL.md is not a name_collision peel: {loser_stdout}"
    );

    let parse = corpus().join("agentskills/invalid-name/Bad_Name/SKILL.md");
    let (_home, mut validate_parse) = bin();
    let parse_out = validate_parse
        .arg("validate")
        .arg("--json")
        .arg(&parse)
        .output()
        .expect("run");
    assert_eq!(parse_out.status.code(), Some(1), "invalid name must fail");
    let parse_stdout = String::from_utf8_lossy(&parse_out.stdout);
    let parse_v: serde_json::Value = serde_json::from_str(&parse_stdout).expect("validate json");
    assert_eq!(parse_v["error_kind"], "parse_error", "{parse_stdout}");
    assert!(
        parse_v.get("winner_path").is_none() && parse_v.get("winnerPath").is_none(),
        "validate --json parse_error must omit winner_path like MCP unknown: {parse_stdout}"
    );
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
fn present_null_user_invocable_peels_parse_error_not_unknown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    let skill = pkg.join("SKILL.md");
    fs::write(
        &skill,
        "---\nname: demo\ndescription: d\nuser_invocable: null\n---\nbody\n",
    )
    .expect("write");

    let (_home, mut load) = bin();
    let load_out = load
        .arg("load")
        .arg("demo")
        .arg("--path")
        .arg(tmp.path())
        .output()
        .expect("run");
    assert_eq!(
        load_out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&load_out.stderr)
    );
    let load_err = String::from_utf8_lossy(&load_out.stderr);
    assert!(
        load_err.contains("skipped skill: demo"),
        "load must name the skipped package: {load_err}"
    );
    assert!(
        load_err.contains("parse_error"),
        "load must include skip kind: {load_err}"
    );
    assert!(
        !load_err.contains("unknown skill"),
        "present-null must not look missing: {load_err}"
    );

    let (_home, mut why) = bin();
    let why_out = why
        .arg("why")
        .arg("demo")
        .arg("--json")
        .arg("--path")
        .arg(tmp.path())
        .output()
        .expect("run");
    assert_eq!(
        why_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&why_out.stderr)
    );
    let why_stdout = String::from_utf8_lossy(&why_out.stdout);
    let why_v: serde_json::Value = serde_json::from_str(&why_stdout).expect("why json");
    assert_eq!(
        why_v["skips"][0]["kind"], "parse_error",
        "why JSON must keep parse_error: {why_stdout}"
    );
    assert_eq!(
        why_v["skips"][0]["code"], "parse_error",
        "why JSON must include machine code: {why_stdout}"
    );
    let skip_path = why_v["skips"][0]["path"].as_str().expect("skip path");
    assert_eq!(
        Path::new(skip_path),
        skill.as_path(),
        "why JSON must keep the SKILL.md path: {why_stdout}"
    );
    assert!(!String::from_utf8_lossy(&why_out.stderr).contains("unknown skill"));

    let (_home, mut validate) = bin();
    let val_out = validate
        .arg("validate")
        .arg("--json")
        .arg(&skill)
        .output()
        .expect("run");
    assert_eq!(val_out.status.code(), Some(1), "present-null must fail");
    let val_stdout = String::from_utf8_lossy(&val_out.stdout);
    let val_v: serde_json::Value = serde_json::from_str(&val_stdout).expect("validate json");
    assert_eq!(
        val_v["error_kind"], "parse_error",
        "validate --json must peel parse_error: {val_stdout}"
    );
    assert_eq!(
        val_v["path"].as_str().map(Path::new),
        Some(skill.as_path()),
        "validate --json must keep the SKILL.md path: {val_stdout}"
    );
    assert!(
        val_v["error"]
            .as_str()
            .is_some_and(|e| e.contains("user_invocable") && e.contains("boolean")),
        "validate peel must name the present-null bool: {val_stdout}"
    );
}

#[test]
fn present_null_disable_model_invocation_peels_parse_error_not_unknown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let pkg = tmp.path().join("demo");
    fs::create_dir_all(&pkg).expect("mkdir");
    let skill = pkg.join("SKILL.md");
    fs::write(
        &skill,
        "---\nname: demo\ndescription: d\ndisable_model_invocation: null\n---\nbody\n",
    )
    .expect("write");

    let (_home, mut load) = bin();
    let load_out = load
        .arg("load")
        .arg("demo")
        .arg("--path")
        .arg(tmp.path())
        .output()
        .expect("run");
    assert_eq!(
        load_out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&load_out.stderr)
    );
    let load_err = String::from_utf8_lossy(&load_out.stderr);
    assert!(
        load_err.contains("skipped skill: demo"),
        "load must name the skipped package: {load_err}"
    );
    assert!(
        load_err.contains("parse_error"),
        "load must include skip kind: {load_err}"
    );
    assert!(
        !load_err.contains("unknown skill"),
        "present-null must not look missing: {load_err}"
    );

    let (_home, mut why) = bin();
    let why_out = why
        .arg("why")
        .arg("demo")
        .arg("--json")
        .arg("--path")
        .arg(tmp.path())
        .output()
        .expect("run");
    assert_eq!(
        why_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&why_out.stderr)
    );
    let why_stdout = String::from_utf8_lossy(&why_out.stdout);
    let why_v: serde_json::Value = serde_json::from_str(&why_stdout).expect("why json");
    assert_eq!(
        why_v["skips"][0]["kind"], "parse_error",
        "why JSON must keep parse_error: {why_stdout}"
    );
    assert_eq!(
        why_v["skips"][0]["code"], "parse_error",
        "why JSON must include machine code: {why_stdout}"
    );
    let skip_path = why_v["skips"][0]["path"].as_str().expect("skip path");
    assert_eq!(
        Path::new(skip_path),
        skill.as_path(),
        "why JSON must keep the SKILL.md path: {why_stdout}"
    );
    assert!(!String::from_utf8_lossy(&why_out.stderr).contains("unknown skill"));

    let (_home, mut validate) = bin();
    let val_out = validate
        .arg("validate")
        .arg("--json")
        .arg(&skill)
        .output()
        .expect("run");
    assert_eq!(val_out.status.code(), Some(1), "present-null must fail");
    let val_stdout = String::from_utf8_lossy(&val_out.stdout);
    let val_v: serde_json::Value = serde_json::from_str(&val_stdout).expect("validate json");
    assert_eq!(
        val_v["error_kind"], "parse_error",
        "validate --json must peel parse_error: {val_stdout}"
    );
    assert_eq!(
        val_v["path"].as_str().map(Path::new),
        Some(skill.as_path()),
        "validate --json must keep the SKILL.md path: {val_stdout}"
    );
    assert!(
        val_v["error"]
            .as_str()
            .is_some_and(|e| e.contains("disable_model_invocation") && e.contains("boolean")),
        "validate peel must name the present-null bool: {val_stdout}"
    );
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
fn why_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the CLI why door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus().join("leftover/empty-nested-skills");
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("why")
        .arg("wanted")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {stdout}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {stdout}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "empty extra/skills is not a skip: {stdout}"
    );
}

#[test]
fn why_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the CLI why door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus().join("leftover/skills-named-package");
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("why")
        .arg("wanted")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {stdout}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {stdout}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "named extra/skills is not a leftover root file: {stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("why")
        .arg("skills")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    assert!(
        loaded.iter().any(|s| s["name"] == "skills"),
        "why must name the extra/skills package: {stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("why")
        .arg("evil")
        .arg("--json")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_ne!(
        out.status.code(),
        Some(0),
        "nested evil must not load: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn load_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the CLI load door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus().join("leftover/empty-nested-skills");
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("load")
        .arg("wanted")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
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
        stdout.contains("[Activated skill: wanted]"),
        "empty extra/skills must not hide leftover sibling wanted: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load leftover sibling body: {stdout}"
    );
}

#[test]
fn load_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the CLI load door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus().join("leftover/skills-named-package");
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("load")
        .arg("wanted")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
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
        stdout.contains("[Activated skill: wanted]"),
        "named extra/skills must not hide leftover sibling wanted: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load leftover sibling body: {stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("load")
        .arg("skills")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
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
        stdout.contains("[Activated skill: skills]"),
        "named extra/skills package must load: {stdout}"
    );
    assert!(
        stdout.contains("PACKAGE_BODY"),
        "must load named extra/skills body: {stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("load")
        .arg("evil")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert_ne!(
        out.status.code(),
        Some(0),
        "nested evil must not load: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
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

#[cfg(unix)]
#[test]
fn load_escaped_extra_path_root_does_not_hide_sibling() {
    let extra = tempfile::tempdir().expect("extra");
    let outside = tempfile::tempdir().expect("out");
    fs::write(
        outside.path().join("secret.md"),
        "---\nname: stolen\ndescription: leaked\n---\nSECRET_BODY\n",
    )
    .expect("write");
    std::os::unix::fs::symlink(
        outside.path().join("secret.md"),
        extra.path().join("SKILL.md"),
    )
    .expect("symlink");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
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
        "sibling public must load: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the escaped root file: {stdout}"
    );
    assert!(
        !stdout.contains("SECRET_BODY"),
        "must not load the escaped SKILL.md body: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn load_escaped_extra_path_skills_does_not_hide_sibling() {
    let extra = tempfile::tempdir().expect("extra");
    let outside = tempfile::tempdir().expect("out");
    fs::create_dir_all(outside.path().join("stolen")).expect("mkdir");
    fs::write(
        outside.path().join("stolen").join("SKILL.md"),
        "---\nname: stolen\ndescription: leaked\n---\nSECRET_BODY\n",
    )
    .expect("write");
    std::os::unix::fs::symlink(outside.path(), extra.path().join("skills")).expect("symlink");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
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
        "sibling public must load when extra/skills/ escapes: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the escaped skills/ tree: {stdout}"
    );
    assert!(
        !stdout.contains("SECRET_BODY"),
        "must not load the escaped skills/ body: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn load_unreadable_extra_path_skills_does_not_hide_sibling() {
    use std::os::unix::fs::PermissionsExt;
    let extra = tempfile::tempdir().expect("extra");
    let skills = extra.path().join("skills");
    fs::create_dir_all(skills.join("hidden")).expect("mkdir");
    fs::write(
        skills.join("hidden").join("SKILL.md"),
        "---\nname: hidden\ndescription: locked\n---\nlocked\n",
    )
    .expect("write");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
    )
    .expect("write");
    let original = fs::metadata(&skills).expect("meta").permissions();
    struct Restore<'a>(&'a std::path::Path, fs::Permissions);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = fs::set_permissions(self.0, self.1.clone());
        }
    }
    let _restore = Restore(&skills, original.clone());
    let mut locked = original.clone();
    locked.set_mode(0o000);
    fs::set_permissions(&skills, locked).expect("chmod");
    if fs::read_dir(&skills).is_ok() {
        return;
    }
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
        "sibling public must load when extra/skills/ is unreadable: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the locked skills/ tree: {stdout}"
    );
}

#[cfg(unix)]
fn mkfifo(path: &std::path::Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo {path:?} failed: {status}");
}

#[cfg(unix)]
#[test]
fn load_fifo_leftover_extra_path_skill_md_does_not_hide_skills_subdir() {
    let extra = tempfile::tempdir().expect("extra");
    mkfifo(&extra.path().join("SKILL.md"));
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
        "skills/public must load when leftover extra/SKILL.md is a FIFO: {stdout}"
    );
    assert!(
        stdout.contains("from-skills"),
        "must load the skills/ package, not the leftover FIFO: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn load_fifo_leftover_and_skills_file_does_not_hide_sibling() {
    let extra = tempfile::tempdir().expect("extra");
    mkfifo(&extra.path().join("SKILL.md"));
    fs::write(extra.path().join("skills"), "not-a-dir").expect("skills file");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
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
        "sibling public must load when leftover extra/SKILL.md is a FIFO and extra/skills is a file: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the leftover FIFO: {stdout}"
    );
}

#[test]
fn load_nameless_leftover_and_skills_file_does_not_hide_sibling() {
    let extra = tempfile::tempdir().expect("extra");
    fs::write(
        extra.path().join("SKILL.md"),
        "---\ndescription: leftover without name\n---\nloose\n",
    )
    .expect("write");
    fs::write(extra.path().join("skills"), "not-a-dir").expect("skills file");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
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
        "sibling public must load when leftover extra/SKILL.md has no name and extra/skills is a file: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the nameless leftover: {stdout}"
    );
}

#[test]
fn load_blank_peek_leftover_and_skills_file_does_not_hide_sibling() {
    let extra = tempfile::tempdir().expect("extra");
    fs::write(
        extra.path().join("SKILL.md"),
        "---\nname: \"   \"\ndescription: leftover blank name\n---\nloose\n",
    )
    .expect("write");
    fs::write(extra.path().join("skills"), "not-a-dir").expect("skills file");
    let pkg = extra.path().join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
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
        "sibling public must load when leftover extra/SKILL.md peeks a blank name and extra/skills is a file: {stdout}"
    );
    assert!(
        stdout.contains("from-sibling"),
        "must load the sibling package, not the blank-peek leftover: {stdout}"
    );
}

#[test]
fn load_invalid_matching_peek_leftover_and_skills_dir_does_not_hide_collection() {
    let extra_root = tempfile::tempdir().expect("extra-root");
    let extra = extra_root.path().join("demo");
    fs::create_dir_all(&extra).expect("mkdir extra");
    fs::write(
        extra.join("SKILL.md"),
        "---\nname: DEMO\ndescription: leftover invalid name\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.join("skills").join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("public")
        .arg("--path")
        .arg(&extra)
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
        "skills/public must load when leftover extra/SKILL.md peeks DEMO matching extra dir: {stdout}"
    );
    assert!(
        stdout.contains("from-collection"),
        "must load the skills/ package, not the invalid-matching leftover: {stdout}"
    );
}

#[test]
fn load_ascii_matching_peek_leftover_and_skills_dir_does_not_hide_collection() {
    let extra_root = tempfile::tempdir().expect("extra-root");
    let extra = extra_root.path().join("café");
    fs::create_dir_all(&extra).expect("mkdir extra");
    fs::write(
        extra.join("SKILL.md"),
        "---\nname: café\ndescription: leftover unicode name\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.join("skills").join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("public")
        .arg("--path")
        .arg(&extra)
        .arg("--ascii-names")
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
        "skills/public must load when leftover extra/SKILL.md peeks café matching extra dir under --ascii-names: {stdout}"
    );
    assert!(
        stdout.contains("from-collection"),
        "must load the skills/ package, not the ascii-matching leftover: {stdout}"
    );
}

#[test]
fn load_unparseable_matching_peek_leftover_and_skills_dir_does_not_hide_collection() {
    let extra_root = tempfile::tempdir().expect("extra-root");
    let extra = extra_root.path().join("demo");
    fs::create_dir_all(&extra).expect("mkdir extra");
    fs::write(extra.join("SKILL.md"), "---\nname: demo\n---\nloose\n").expect("write");
    let pkg = extra.join("skills").join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("public")
        .arg("--path")
        .arg(&extra)
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
        "skills/public must load when leftover extra/SKILL.md peeks demo matching extra dir but cannot parse: {stdout}"
    );
    assert!(
        stdout.contains("from-collection"),
        "must load the skills/ package, not the unparseable leftover: {stdout}"
    );
}

#[test]
fn load_path_component_peek_leftover_and_skills_dir_does_not_hide_collection() {
    let extra_root = tempfile::tempdir().expect("extra-root");
    let extra = extra_root.path().join("wanted");
    fs::create_dir_all(&extra).expect("mkdir extra");
    fs::write(
        extra.join("SKILL.md"),
        "---\nname: .\ndescription: leftover path-like name\n---\nloose\n",
    )
    .expect("write");
    let pkg = extra.join("skills").join("public");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("public")
        .arg("--path")
        .arg(&extra)
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
        "skills/public must load when leftover extra/SKILL.md peeks `.`: {stdout}"
    );
    assert!(
        stdout.contains("from-collection"),
        "must load the skills/ package, not the path-component leftover: {stdout}"
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
fn load_includes_when_to_use() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("ranked");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: ranked\ndescription: hinted\nwhen-to-use: after rebase\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-when");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-when\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("ranked")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("When to use: after rebase"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("no-when")
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
        stdout.contains("[Activated skill: no-when]"),
        "bare skill must still load: {stdout}"
    );
    assert!(
        !stdout.contains("When to use:"),
        "omitted when_to_use must not add a load line: {stdout}"
    );
}

#[test]
fn load_includes_triggers() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("triggered");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: triggered\ndescription: hinted\ntriggers: git, A & B\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-triggers");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-triggers\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("triggered")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Triggers: git, A & B"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("no-triggers")
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
        stdout.contains("[Activated skill: no-triggers]"),
        "bare skill must still load: {stdout}"
    );
    assert!(
        !stdout.contains("Triggers:"),
        "empty triggers must not add a load line: {stdout}"
    );
}

#[test]
fn load_includes_metadata() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("annotated");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: annotated\ndescription: hinted\nmetadata:\n  author: craftbag\n  version: \"1.0\"\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-metadata");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-metadata\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("annotated")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Metadata: author=craftbag, version=1.0",
        ));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("no-metadata")
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
        stdout.contains("[Activated skill: no-metadata]"),
        "bare skill must still load: {stdout}"
    );
    assert!(
        !stdout.contains("Metadata:"),
        "empty metadata must not add a load line: {stdout}"
    );
}

#[test]
fn load_includes_argument_hint() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("slash-hint");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: slash-hint\ndescription: hinted\nargument-hint: [name]\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-hint");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-hint\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("slash-hint")
        .arg("--args")
        .arg("alice")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Argument hint: [name]"))
        .stdout(predicates::str::contains("User arguments: alice"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("no-hint")
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
        stdout.contains("[Activated skill: no-hint]"),
        "bare skill must still load: {stdout}"
    );
    assert!(
        !stdout.contains("Argument hint:"),
        "omitted argument_hint must not add a load line: {stdout}"
    );
}

#[test]
fn load_includes_allowed_tools() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("tools-ok");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: tools-ok\ndescription: hinted\nallowed-tools: Read Bash(git:*)\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-tools");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-tools\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("tools-ok")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Allowed tools: Read Bash(git:*)"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("no-tools")
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
        stdout.contains("[Activated skill: no-tools]"),
        "bare skill must still load: {stdout}"
    );
    assert!(
        !stdout.contains("Allowed tools:"),
        "omitted allowed_tools must not add a load line: {stdout}"
    );
}

#[test]
fn load_flattens_multiline_description() {
    let extra = tempfile::tempdir().expect("extra");
    let pkg = extra.path().join("lit-skill");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: lit-skill\ndescription: |\n  line one\n  line two\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("lit-skill")
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
    let header = stdout.split("\n---\n").next().expect("header");
    assert!(
        header.contains("Description: line one line two\n"),
        "load must fold a `|` description to one envelope line: {stdout}"
    );
    assert!(
        !header.contains("line one\nline two"),
        "raw `|` description must not split the envelope: {stdout}"
    );
}

#[test]
fn load_unicode_name_skips_with_ascii_names() {
    let extra = tempfile::tempdir().expect("extra");
    let pkg = extra.path().join("café");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: café\ndescription: coffee\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("café")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("[Activated skill: café]"));
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("café")
        .arg("--path")
        .arg(extra.path())
        .arg("--ascii-names")
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
        stderr.contains("parse_error"),
        "ascii-names must skip café: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn load_explicit_extra_path_skill_md_file_symlink() {
    let outside = tempfile::tempdir().expect("out");
    let pkg = outside.path().join("wanted");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(
        pkg.join("SKILL.md"),
        "---\nname: wanted\ndescription: extra file\n---\nhost asked\n",
    )
    .expect("write");
    let link = tempfile::tempdir().expect("link");
    let dest_dir = link.path().join("wanted");
    fs::create_dir_all(&dest_dir).expect("mkdir");
    let dest = dest_dir.join("SKILL.md");
    std::os::unix::fs::symlink(pkg.join("SKILL.md"), &dest).expect("symlink");
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("wanted")
        .arg("--path")
        .arg(&dest)
        .assert()
        .success()
        .stdout(predicates::str::contains("[Activated skill: wanted]"))
        .stdout(predicates::str::contains("host asked"));
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("wanted")
        .arg("--path")
        .arg(&dest)
        .assert()
        .success()
        .stdout(predicates::str::contains("loaded\twanted"));
}

#[test]
fn list_unknown_vendor_is_rejected() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("list")
        .arg("--vendor")
        .arg("nope")
        .output()
        .expect("run");
    assert_ne!(
        out.status.code(),
        Some(0),
        "unknown --vendor must not look like an empty catalog: stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown vendor: nope"),
        "must name the bad token: {stderr}"
    );
    assert!(
        stderr.contains("claude"),
        "must list valid vendor tokens: {stderr}"
    );
}

#[test]
fn list_vendor_extra_token_is_rejected() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(tmp.path())
        .arg("list")
        .arg("--vendor")
        .arg("extra")
        .output()
        .expect("run");
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown vendor: extra"),
        "extra is --path, not --vendor: {stderr}"
    );
}

#[test]
fn list_vendor_leading_dot_loads_claude() {
    let cwd = corpus().join("incumbent/claude-project");
    let (_home, mut cmd) = bin();
    cmd.current_dir(&cwd)
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg(".claude")
        .assert()
        .success()
        .stdout(predicates::str::contains("pdf-helper"));
}

#[test]
fn list_vendor_cursor_loads_project_layout() {
    let cwd = corpus().join("incumbent/cursor-project");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg("cursor")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "create-rule"),
        "cursor",
        "vendor JSON source must be the wire token: {stdout}"
    );
}

#[test]
fn list_vendor_grok_loads_project_layout() {
    let cwd = corpus().join("incumbent/grok-project");
    let skill = cwd.join(".grok/skills/project-grok/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Grok project fixture must exist: {}",
        skill.display()
    );
    let (_home, mut off) = bin();
    let off_out = off
        .current_dir(&cwd)
        .arg("list")
        .arg("--json")
        .output()
        .expect("run");
    assert_eq!(
        off_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&off_out.stderr)
    );
    let off_stdout = String::from_utf8_lossy(&off_out.stdout);
    let off_v: serde_json::Value = serde_json::from_str(&off_stdout).expect("list json");
    let off_skills = off_v["skills"].as_array().expect("skills");
    assert!(
        off_skills.iter().all(|s| s["name"] != "project-grok"),
        "grok vendor is opt-in: {off_stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg("grok")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "project-grok"),
        "grok",
        "vendor JSON source must be the wire token: {stdout}"
    );
}

#[test]
fn list_watch_dirs_vendor_cursor_lists_project_skills() {
    let cwd = corpus().join("incumbent/cursor-project");
    let want = cwd
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize cursor-project: {e}"))
        .join(".cursor")
        .join("skills");
    assert!(
        want.is_dir(),
        "committed Cursor project skills dir must exist: {}",
        want.display()
    );
    let (_home, mut off) = bin();
    let off_out = off
        .current_dir(&cwd)
        .arg("list")
        .arg("--watch-dirs")
        .output()
        .expect("run");
    assert_eq!(
        off_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&off_out.stderr)
    );
    let off_stdout = String::from_utf8_lossy(&off_out.stdout);
    assert!(
        !stdout_has_path(&off_stdout, &want),
        "watch-dirs without --vendor cursor must not list .cursor/skills: {off_stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("list")
        .arg("--watch-dirs")
        .arg("--vendor")
        .arg("cursor")
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
        stdout_has_path(&stdout, &want),
        "watch-dirs --vendor cursor must list .cursor/skills: {stdout}"
    );
}

#[test]
fn list_watch_dirs_vendor_grok_lists_project_skills() {
    let cwd = corpus().join("incumbent/grok-project");
    let want = cwd
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize grok-project: {e}"))
        .join(".grok")
        .join("skills");
    assert!(
        want.is_dir(),
        "committed Grok project skills dir must exist: {}",
        want.display()
    );
    let (_home, mut off) = bin();
    let off_out = off
        .current_dir(&cwd)
        .arg("list")
        .arg("--watch-dirs")
        .output()
        .expect("run");
    assert_eq!(
        off_out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&off_out.stderr)
    );
    let off_stdout = String::from_utf8_lossy(&off_out.stdout);
    assert!(
        !stdout_has_path(&off_stdout, &want),
        "watch-dirs without --vendor grok must not list .grok/skills: {off_stdout}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("list")
        .arg("--watch-dirs")
        .arg("--vendor")
        .arg("grok")
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
        stdout_has_path(&stdout, &want),
        "watch-dirs --vendor grok must list .grok/skills: {stdout}"
    );
}

#[test]
fn list_watch_dirs_leftover_empty_nested_skills_lists_extra_root() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the CLI watch door. Default vendor stays off. Empty extra/skills
    // is not a discover walk.
    let extra = corpus()
        .join("leftover/empty-nested-skills")
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize leftover extra: {e}"));
    let extra_skills = extra.join("skills");
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    assert!(
        extra_skills.is_dir(),
        "committed leftover empty extra/skills dir must exist: {}",
        extra_skills.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
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
        stdout_has_path(&stdout, &extra),
        "watch-dirs must list leftover extra-path root: {stdout}"
    );
    assert!(
        !stdout_has_path(&stdout, &extra_skills),
        "empty extra/skills is not a discover walk: {stdout}"
    );
    assert!(
        !stdout.contains("wanted") && !stdout.contains("## Skills"),
        "watch-dirs must not load SKILL.md: {stdout}"
    );
}

#[test]
fn list_watch_dirs_leftover_skills_named_package_lists_extra_root() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the CLI watch door. Default vendor stays off. Named extra/skills
    // is a package, not a discover walk.
    let extra = corpus()
        .join("leftover/skills-named-package")
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize leftover extra: {e}"));
    let extra_skills = extra.join("skills");
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra_skills.join("SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(cwd.path())
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(&extra)
        .arg("--no-implicit-roots")
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
        stdout_has_path(&stdout, &extra),
        "watch-dirs must list leftover extra-path root: {stdout}"
    );
    assert!(
        !stdout_has_path(&stdout, &extra_skills),
        "named extra/skills is not a discover walk: {stdout}"
    );
    assert!(
        !stdout.contains("wanted") && !stdout.contains("evil") && !stdout.contains("## Skills"),
        "watch-dirs must not load SKILL.md: {stdout}"
    );
}

#[test]
fn why_vendor_cursor_names_create_rule() {
    let cwd = corpus().join("incumbent/cursor-project");
    let skill = cwd.join(".cursor/skills/create-rule/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Cursor project fixture must exist: {}",
        skill.display()
    );
    let (_home, mut off) = bin();
    let off_out = off
        .current_dir(&cwd)
        .arg("why")
        .arg("create-rule")
        .arg("--json")
        .output()
        .expect("run");
    assert_eq!(
        off_out.status.code(),
        Some(1),
        "cursor vendor is opt-in: stderr={}",
        String::from_utf8_lossy(&off_out.stderr)
    );
    let off_err = String::from_utf8_lossy(&off_out.stderr);
    assert!(
        off_err.contains("unknown skill: create-rule"),
        "why without --vendor cursor must not see create-rule: {off_err}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("why")
        .arg("create-rule")
        .arg("--json")
        .arg("--vendor")
        .arg("cursor")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "create-rule")
        .unwrap_or_else(|| panic!("why must name create-rule: {stdout}"));
    assert_eq!(
        row["source"], "cursor",
        "why JSON source must be the wire token: {stdout}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "create-rule is a loaded vendor skill, not a skip: {stdout}"
    );
}

#[test]
fn why_vendor_grok_names_project_grok() {
    let cwd = corpus().join("incumbent/grok-project");
    let skill = cwd.join(".grok/skills/project-grok/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Grok project fixture must exist: {}",
        skill.display()
    );
    let (_home, mut off) = bin();
    let off_out = off
        .current_dir(&cwd)
        .arg("why")
        .arg("project-grok")
        .arg("--json")
        .output()
        .expect("run");
    assert_eq!(
        off_out.status.code(),
        Some(1),
        "grok vendor is opt-in: stderr={}",
        String::from_utf8_lossy(&off_out.stderr)
    );
    let off_err = String::from_utf8_lossy(&off_out.stderr);
    assert!(
        off_err.contains("unknown skill: project-grok"),
        "why without --vendor grok must not see project-grok: {off_err}"
    );
    let (_home, mut cmd) = bin();
    let out = cmd
        .current_dir(&cwd)
        .arg("why")
        .arg("project-grok")
        .arg("--json")
        .arg("--vendor")
        .arg("grok")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "project-grok")
        .unwrap_or_else(|| panic!("why must name project-grok: {stdout}"));
    assert_eq!(
        row["source"], "grok",
        "why JSON source must be the wire token: {stdout}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "project-grok is a loaded vendor skill, not a skip: {stdout}"
    );
}

#[test]
fn list_vendor_claude_loads_user_home_layout() {
    let home = corpus().join("incumbent/claude-user");
    let cwd = tempfile::tempdir().expect("cwd");
    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let out = cmd
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg("claude")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "home-note"),
        "claude",
        "vendor JSON source must be the wire token, not a vendor object: {stdout}"
    );
}

#[test]
fn list_vendor_cursor_loads_user_home_layout() {
    let home = corpus().join("incumbent/cursor-user");
    let cwd = tempfile::tempdir().expect("cwd");
    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let out = cmd
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg("cursor")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "home-rule"),
        "cursor",
        "vendor JSON source must be the wire token, not a vendor object: {stdout}"
    );
}

#[test]
fn list_vendor_grok_loads_user_home_layout() {
    let home = corpus().join("incumbent/grok-user");
    let cwd = tempfile::tempdir().expect("cwd");
    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let out = cmd
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--vendor")
        .arg("grok")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        list_json_source(&stdout, "home-grok"),
        "grok",
        "vendor JSON source must be the wire token, not a vendor object: {stdout}"
    );
}

#[test]
fn list_help_names_vendor_path_examples() {
    let (_home, mut cmd) = bin();
    let stdout = String::from_utf8_lossy(
        &cmd.arg("list")
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .into_owned();
    assert!(stdout.contains("--vendor"), "{stdout}");
    for token in craftbag::SkillSource::VENDOR_TOKENS {
        assert!(
            stdout.contains(token),
            "list --help must name vendor token {token}: {stdout}"
        );
    }
    assert!(stdout.contains("--path"), "{stdout}");
    assert!(stdout.contains("--format"), "{stdout}");
    assert!(stdout.contains("json"), "{stdout}");
    assert!(stdout.contains("xml"), "{stdout}");
    assert!(stdout.contains("catalog"), "{stdout}");
    assert!(stdout.contains("watch"), "{stdout}");
    assert!(stdout.contains("watch-dirs"), "{stdout}");
    assert!(stdout.contains("watch_dirs"), "{stdout}");
    assert!(stdout.contains("Example:"), "{stdout}");
    for cmd_name in ["load", "why"] {
        let (_home, mut cmd) = bin();
        let help = String::from_utf8_lossy(
            &cmd.arg(cmd_name)
                .arg("--help")
                .assert()
                .success()
                .get_output()
                .stdout,
        )
        .into_owned();
        assert!(help.contains("--vendor"), "{cmd_name}: {help}");
        for token in craftbag::SkillSource::VENDOR_TOKENS {
            assert!(
                help.contains(token),
                "{cmd_name} --help must name vendor token {token}: {help}"
            );
        }
    }
}

#[test]
fn list_help_names_json_skills_skips() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--json"))
        .stdout(predicates::str::contains("{ skills, skips }"));
}

#[test]
fn list_help_format_names_json_skills_skips() {
    let (_home, mut cmd) = bin();
    let stdout = String::from_utf8_lossy(
        &cmd.arg("list")
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .into_owned();
    let lines: Vec<&str> = stdout.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("--format"))
        .expect("list --help must name --format");
    let mut block = vec![lines[start]];
    for line in &lines[start + 1..] {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.starts_with("-h") || trimmed.is_empty() {
            break;
        }
        block.push(*line);
    }
    let format_help = block.join("\n");
    assert!(
        format_help.contains("{ skills, skips }"),
        "list --format must name json keys like MCP skills_list format: {format_help}"
    );
    assert!(
        format_help.contains("<available_skills>"),
        "list --format must name xml <available_skills> like MCP skills_list format: {format_help}"
    );
    for token in craftbag::ListFormat::CANONICAL_TOKENS {
        assert!(
            format_help.contains(token),
            "list --format help must name canonical token {token}: {format_help}"
        );
    }
}

#[test]
fn why_help_names_json_error_kind() {
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--json"))
        .stdout(predicates::str::contains("error_kind"))
        .stdout(predicates::str::contains("{ error_kind, error }"));
}

#[test]
fn why_help_names_json_winner_path() {
    // SkillMiss.winner_path landed in #191. why --help must name the key so
    // a leftover host does not scrape `lost to` from Display.
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("winner_path"))
        .stdout(predicates::str::contains("name_collision"));
}

#[test]
fn why_help_names_json_path() {
    // SkillMiss.path landed in #139. `--path` is the extra-root flag, so
    // a leftover host must see `path when a skip` to peel the SKILL.md
    // instead of scraping `at ` from Display.
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("`path` when a skip"));
}

#[test]
fn validate_help_names_json_error_kind() {
    let (_home, mut cmd) = bin();
    cmd.arg("validate")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--json"))
        .stdout(predicates::str::contains("error_kind"))
        .stdout(predicates::str::contains("{ error_kind, error }"))
        .stdout(predicates::str::contains("plus `path`"))
        .stdout(predicates::str::contains("winner_path"))
        .stdout(predicates::str::contains("name_collision"));
}

#[test]
fn list_help_names_no_implicit_roots() {
    for cmd_name in ["list", "load", "why"] {
        let (_home, mut cmd) = bin();
        let help = String::from_utf8_lossy(
            &cmd.arg(cmd_name)
                .arg("--help")
                .assert()
                .success()
                .get_output()
                .stdout,
        )
        .into_owned();
        assert!(
            help.contains("--no-implicit-roots"),
            "{cmd_name} --help must name --no-implicit-roots: {help}"
        );
        assert!(
            help.contains("cwd-to-git .agents"),
            "{cmd_name} --help must attach .agents to cwd-to-git, not walk the whole tree: {help}"
        );
        assert!(
            help.contains("Default is on"),
            "{cmd_name} --help must name implicit_roots default on: {help}"
        );
        // `--path` / `--user-dir` also appear as their own flags, so
        // a lone contains would pass if implicit-roots help said they
        // do not load.
        assert!(
            help.contains("Extra --path and --user-dir still load"),
            "{cmd_name} --help must say extra --path / --user-dir still load (not an inverted sentence): {help}"
        );
    }
}

#[test]
fn list_no_implicit_roots_skips_cwd_and_home() {
    let cwd = tempfile::tempdir().expect("cwd");
    fs::create_dir_all(cwd.path().join(".git")).expect("git");
    let leaked = cwd.path().join(".agents").join("skills").join("leaked");
    fs::create_dir_all(&leaked).expect("leaked");
    fs::write(
        leaked.join("SKILL.md"),
        "---\nname: leaked\ndescription: from-cwd\n---\nFROM_CWD\n",
    )
    .expect("write leaked");
    let extra = tempfile::tempdir().expect("extra");
    let wanted = extra.path().join("wanted");
    fs::create_dir_all(&wanted).expect("wanted");
    fs::write(
        wanted.join("SKILL.md"),
        "---\nname: wanted\ndescription: from-extra\n---\nFROM_EXTRA\n",
    )
    .expect("write wanted");
    let home = tempfile::tempdir().expect("home");
    let homeskill = home.path().join(".agents").join("skills").join("homeskill");
    fs::create_dir_all(&homeskill).expect("homeskill");
    fs::write(
        homeskill.join("SKILL.md"),
        "---\nname: homeskill\ndescription: from-home\n---\nFROM_HOME\n",
    )
    .expect("write homeskill");

    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let off = cmd
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert!(
        off.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&off.stderr)
    );
    let off_out = String::from_utf8_lossy(&off.stdout);
    assert_eq!(
        list_json_source(&off_out, "wanted"),
        "extra",
        "collection-only must load extra wanted: {off_out}"
    );
    let v: serde_json::Value = serde_json::from_str(&off_out).expect("json");
    let names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["wanted"],
        "collection-only must not leak cwd/HOME: {off_out}"
    );

    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let on = cmd
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .current_dir(cwd.path())
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .output()
        .expect("run");
    assert!(
        on.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&on.stderr)
    );
    let on_out = String::from_utf8_lossy(&on.stdout);
    assert!(
        on_out.contains("leaked") && on_out.contains("homeskill") && on_out.contains("wanted"),
        "default list --path must stay additive: {on_out}"
    );
}

#[test]
fn list_watch_dirs_no_implicit_roots_omits_cwd_and_home() {
    let cwd = tempfile::tempdir().expect("cwd");
    fs::create_dir_all(cwd.path().join(".git")).expect("git");
    let agents = cwd.path().join(".agents").join("skills");
    fs::create_dir_all(&agents).expect("agents");
    let extra = tempfile::tempdir().expect("extra");
    fs::create_dir_all(extra.path().join("wanted")).expect("wanted");
    fs::write(
        extra.path().join("wanted").join("SKILL.md"),
        "---\nname: wanted\ndescription: from-extra\n---\nFROM_EXTRA\n",
    )
    .expect("write");
    let home = tempfile::tempdir().expect("home");
    let home_agents = home.path().join(".agents").join("skills");
    fs::create_dir_all(&home_agents).expect("home agents");

    let mut cmd = Command::cargo_bin("craftbag").expect("bin");
    let out = cmd
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .current_dir(cwd.path())
        .arg("list")
        .arg("--watch-dirs")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout_has_path(&stdout, extra.path()),
        "watch-dirs must still list extra collection: {stdout}"
    );
    assert!(
        !stdout_has_path(&stdout, &agents),
        "watch-dirs --no-implicit-roots must omit cwd .agents: {stdout}"
    );
    assert!(
        !stdout_has_path(&stdout, &home_agents),
        "watch-dirs --no-implicit-roots must omit HOME .agents: {stdout}"
    );
}

#[test]
fn list_help_names_disabled() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--disabled"))
        .stdout(predicates::str::contains("no skip row"));
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--disabled"));
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--disabled"));
}

#[test]
fn list_disabled_omits_named_skill() {
    let extra = tempfile::tempdir().expect("extra");
    let keep = extra.path().join("keep");
    fs::create_dir_all(&keep).expect("keep");
    fs::write(
        keep.join("SKILL.md"),
        "---\nname: keep\ndescription: stay\n---\nKEEP\n",
    )
    .expect("write keep");
    let off = extra.path().join("off");
    fs::create_dir_all(&off).expect("off");
    fs::write(
        off.join("SKILL.md"),
        "---\nname: off\ndescription: hide\n---\nOFF\n",
    )
    .expect("write off");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--disabled")
        .arg("off")
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(names, ["keep"], "disabled off must not appear: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "disabled is silent (no skip row): {stdout}"
    );
}

#[test]
fn load_disabled_is_unknown() {
    let extra = tempfile::tempdir().expect("extra");
    let off = extra.path().join("off");
    fs::create_dir_all(&off).expect("off");
    fs::write(
        off.join("SKILL.md"),
        "---\nname: off\ndescription: hide\n---\nOFF\n",
    )
    .expect("write off");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("off")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--disabled")
        .arg("OFF")
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
        stderr.contains("unknown skill: off"),
        "disabled load must be unknown, not a skip row: {stderr}"
    );
}

#[test]
fn why_disabled_is_unknown() {
    let extra = tempfile::tempdir().expect("extra");
    let off = extra.path().join("off");
    fs::create_dir_all(&off).expect("off");
    fs::write(
        off.join("SKILL.md"),
        "---\nname: off\ndescription: hide\n---\nOFF\n",
    )
    .expect("write off");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
        .arg("off")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--disabled")
        .arg("off")
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["error_kind"], "unknown_skill", "stdout={stdout}");
}

#[test]
fn list_help_names_ignore() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--ignore"))
        .stdout(predicates::str::contains("no skip row"));
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--ignore"));
    let (_home, mut cmd) = bin();
    cmd.arg("why")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--ignore"));
}

#[test]
fn list_ignore_omits_prefix() {
    let extra = tempfile::tempdir().expect("extra");
    let keep = extra.path().join("keep");
    fs::create_dir_all(&keep).expect("keep");
    fs::write(
        keep.join("SKILL.md"),
        "---\nname: keep\ndescription: stay\n---\nKEEP\n",
    )
    .expect("write keep");
    let secret = extra.path().join("secret");
    fs::create_dir_all(&secret).expect("secret");
    fs::write(
        secret.join("SKILL.md"),
        "---\nname: secret\ndescription: hide\n---\nSECRET\n",
    )
    .expect("write secret");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--ignore")
        .arg(&secret)
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(names, ["keep"], "ignored prefix must not appear: {stdout}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(skips.is_empty(), "ignore is silent (no skip row): {stdout}");
}

#[test]
fn load_ignore_is_unknown() {
    let extra = tempfile::tempdir().expect("extra");
    let secret = extra.path().join("secret");
    fs::create_dir_all(&secret).expect("secret");
    fs::write(
        secret.join("SKILL.md"),
        "---\nname: secret\ndescription: hide\n---\nSECRET\n",
    )
    .expect("write secret");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("load")
        .arg("secret")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--ignore")
        .arg(&secret)
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
        stderr.contains("unknown skill: secret"),
        "ignored load must be unknown, not a skip row: {stderr}"
    );
}

#[test]
fn why_ignore_is_unknown() {
    let extra = tempfile::tempdir().expect("extra");
    let secret = extra.path().join("secret");
    fs::create_dir_all(&secret).expect("secret");
    fs::write(
        secret.join("SKILL.md"),
        "---\nname: secret\ndescription: hide\n---\nSECRET\n",
    )
    .expect("write secret");

    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
        .arg("secret")
        .arg("--json")
        .arg("--path")
        .arg(extra.path())
        .arg("--no-implicit-roots")
        .arg("--ignore")
        .arg(&secret)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["error_kind"], "unknown_skill", "stdout={stdout}");
}

#[test]
fn load_help_names_args_and_argument_hint() {
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--args"))
        .stdout(predicates::str::contains("argument-hint"))
        .stdout(predicates::str::contains("when-to-use"))
        .stdout(predicates::str::contains("triggers"))
        .stdout(predicates::str::contains("allowed-tools"))
        .stdout(predicates::str::contains("license"))
        .stdout(predicates::str::contains("compatibility"))
        .stdout(predicates::str::contains("metadata"))
        .stdout(predicates::str::contains("Example:"));
}

#[test]
fn load_help_names_json_error_kind() {
    // SkillMiss peel landed on why/validate first. load --help must name
    // the same keys so a leftover host does not scrape Display.
    let (_home, mut cmd) = bin();
    cmd.arg("load")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--json"))
        .stdout(predicates::str::contains("error_kind"))
        .stdout(predicates::str::contains("{ error_kind, error }"))
        .stdout(predicates::str::contains("`path` when a skip"))
        .stdout(predicates::str::contains("winner_path"))
        .stdout(predicates::str::contains("name_collision"));
}

#[test]
fn list_json_includes_invocation_flags() {
    let extra = tempfile::tempdir().expect("extra");
    let hidden = extra.path().join("hidden-slash");
    fs::create_dir_all(&hidden).expect("mkdir");
    fs::write(
        hidden.join("SKILL.md"),
        "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\ndisable_model_invocation: false\n---\nbody\n",
    )
    .expect("write");
    let slash = extra.path().join("slash-only");
    fs::create_dir_all(&slash).expect("mkdir");
    fs::write(
        slash.join("SKILL.md"),
        "---\nname: slash-only\ndescription: user only\nuser-invocable: true\ndisable-model-invocation: true\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let hidden_row = skills
        .iter()
        .find(|s| s["name"] == "hidden-slash")
        .expect("hidden-slash");
    assert_eq!(
        hidden_row["user_invocable"], false,
        "list JSON must carry user_invocable for slash palettes: {stdout}"
    );
    assert_eq!(hidden_row["disable_model_invocation"], false, "{stdout}");
    let slash_row = skills
        .iter()
        .find(|s| s["name"] == "slash-only")
        .expect("slash-only");
    assert_eq!(slash_row["user_invocable"], true, "{stdout}");
    assert_eq!(slash_row["disable_model_invocation"], true, "{stdout}");
}

#[test]
fn list_json_includes_argument_hint() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("slash-hint");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: slash-hint\ndescription: hinted\nargument-hint: [name]\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-hint");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-hint\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let hinted_row = skills
        .iter()
        .find(|s| s["name"] == "slash-hint")
        .expect("slash-hint");
    assert_eq!(
        hinted_row["argument_hint"], "[name]",
        "list JSON must carry argument_hint for slash palettes: {stdout}"
    );
    assert!(
        hinted_row.get("argumentHint").is_none(),
        "list JSON argument_hint must stay snake_case: {stdout}"
    );
    let bare_row = skills
        .iter()
        .find(|s| s["name"] == "no-hint")
        .expect("no-hint");
    assert!(
        bare_row["argument_hint"].is_null(),
        "omitted argument_hint is null on list JSON: {stdout}"
    );
}

#[test]
fn list_json_includes_when_to_use() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("ranked");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: ranked\ndescription: hinted\nwhen-to-use: after rebase\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-when");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-when\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let hinted_row = skills
        .iter()
        .find(|s| s["name"] == "ranked")
        .expect("ranked");
    assert_eq!(
        hinted_row["when_to_use"], "after rebase",
        "list JSON must carry when_to_use for catalogs: {stdout}"
    );
    assert!(
        hinted_row.get("whenToUse").is_none(),
        "list JSON when_to_use must stay snake_case: {stdout}"
    );
    let bare_row = skills
        .iter()
        .find(|s| s["name"] == "no-when")
        .expect("no-when");
    assert!(
        bare_row["when_to_use"].is_null(),
        "omitted when_to_use is null on list JSON: {stdout}"
    );
}

#[test]
fn list_json_includes_allowed_tools() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("tools-ok");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: tools-ok\ndescription: hinted\nallowed-tools: Read Bash(git:*)\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-tools");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-tools\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let hinted_row = skills
        .iter()
        .find(|s| s["name"] == "tools-ok")
        .expect("tools-ok");
    assert_eq!(
        hinted_row["allowed_tools"], "Read Bash(git:*)",
        "list JSON must carry allowed_tools: {stdout}"
    );
    assert!(
        hinted_row.get("allowedTools").is_none(),
        "list JSON allowed_tools must stay snake_case: {stdout}"
    );
    let bare_row = skills
        .iter()
        .find(|s| s["name"] == "no-tools")
        .expect("no-tools");
    assert!(
        bare_row["allowed_tools"].is_null(),
        "omitted allowed_tools is null on list JSON: {stdout}"
    );
}

#[test]
fn list_json_includes_metadata() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("annotated");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: annotated\ndescription: hinted\nmetadata:\n  author: A & B\n  version: \"1.0\"\n---\nbody\n",
    )
    .expect("write");
    let bare = extra.path().join("no-metadata");
    fs::create_dir_all(&bare).expect("mkdir");
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: no-metadata\ndescription: bare\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let hinted_row = skills
        .iter()
        .find(|s| s["name"] == "annotated")
        .expect("annotated");
    assert_eq!(
        hinted_row["metadata"],
        serde_json::json!({"author": "A & B", "version": "1.0"}),
        "list JSON must carry metadata: {stdout}"
    );
    let bare_row = skills
        .iter()
        .find(|s| s["name"] == "no-metadata")
        .expect("no-metadata");
    assert_eq!(
        bare_row["metadata"],
        serde_json::json!({}),
        "empty metadata is {{}} on list JSON, not omitted: {stdout}"
    );
}

#[test]
fn list_catalog_includes_when_to_use() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("ranked");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: ranked\ndescription: hinted\nwhen-to-use: after rebase\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--catalog")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "- **ranked**: hinted Use when: after rebase",
        ));
}

#[test]
fn list_xml_includes_when_to_use() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("ranked");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: ranked\ndescription: hinted\nwhen-to-use: A & B\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--xml")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "<when_to_use>A &amp; B</when_to_use>",
        ));
}

#[test]
fn list_xml_includes_allowed_tools() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("tools-ok");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: tools-ok\ndescription: hinted\nallowed-tools: Read & Bash\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--xml")
        .arg("--path")
        .arg(extra.path())
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "<allowed_tools>Read &amp; Bash</allowed_tools>",
        ));
}

#[test]
fn why_json_includes_when_to_use() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("ranked");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: ranked\ndescription: hinted\nwhen_to_use: after rebase\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let hinted_row = loaded
        .iter()
        .find(|s| s["name"] == "ranked")
        .expect("ranked");
    assert_eq!(
        hinted_row["when_to_use"], "after rebase",
        "why JSON must carry when_to_use like list JSON/XML: {stdout}"
    );
}

#[test]
fn why_json_includes_argument_hint() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("slash-hint");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: slash-hint\ndescription: hinted\nargument_hint: [name]\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let hinted_row = loaded
        .iter()
        .find(|s| s["name"] == "slash-hint")
        .expect("slash-hint");
    assert_eq!(
        hinted_row["argument_hint"], "[name]",
        "why JSON must carry argument_hint like list JSON/XML: {stdout}"
    );
}

#[test]
fn why_json_includes_allowed_tools() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("tools-ok");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: tools-ok\ndescription: hinted\nallowed_tools: Read Bash(git:*)\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let hinted_row = loaded
        .iter()
        .find(|s| s["name"] == "tools-ok")
        .expect("tools-ok");
    assert_eq!(
        hinted_row["allowed_tools"], "Read Bash(git:*)",
        "why JSON must carry allowed_tools like list JSON/XML: {stdout}"
    );
}

#[test]
fn why_json_includes_metadata() {
    let extra = tempfile::tempdir().expect("extra");
    let hinted = extra.path().join("annotated");
    fs::create_dir_all(&hinted).expect("mkdir");
    fs::write(
        hinted.join("SKILL.md"),
        "---\nname: annotated\ndescription: hinted\nmetadata:\n  author: A & B\n  version: \"1.0\"\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let hinted_row = loaded
        .iter()
        .find(|s| s["name"] == "annotated")
        .expect("annotated");
    assert_eq!(
        hinted_row["metadata"],
        serde_json::json!({"author": "A & B", "version": "1.0"}),
        "why JSON must carry metadata like list JSON/XML: {stdout}"
    );
}

#[test]
fn why_json_includes_invocation_flags() {
    let extra = tempfile::tempdir().expect("extra");
    let hidden = extra.path().join("hidden-slash");
    fs::create_dir_all(&hidden).expect("mkdir");
    fs::write(
        hidden.join("SKILL.md"),
        "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\ndisable_model_invocation: false\n---\nbody\n",
    )
    .expect("write");
    let slash = extra.path().join("slash-only");
    fs::create_dir_all(&slash).expect("mkdir");
    fs::write(
        slash.join("SKILL.md"),
        "---\nname: slash-only\ndescription: user only\nuser-invocable: true\ndisable-model-invocation: true\n---\nbody\n",
    )
    .expect("write");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("why")
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
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let hidden_row = loaded
        .iter()
        .find(|s| s["name"] == "hidden-slash")
        .expect("hidden-slash");
    assert_eq!(
        hidden_row["user_invocable"], false,
        "why JSON must carry user_invocable like list JSON/XML: {stdout}"
    );
    assert_eq!(hidden_row["disable_model_invocation"], false, "{stdout}");
    assert!(
        hidden_row.get("userInvocable").is_none(),
        "why JSON flags must match list snake_case, not Skill camelCase: {stdout}"
    );
    let slash_row = loaded
        .iter()
        .find(|s| s["name"] == "slash-only")
        .expect("slash-only");
    assert_eq!(slash_row["user_invocable"], true, "{stdout}");
    assert_eq!(slash_row["disable_model_invocation"], true, "{stdout}");
    assert_eq!(
        hidden_row["description"], "model only",
        "why JSON must carry description like list JSON/XML: {stdout}"
    );
    assert_eq!(slash_row["description"], "user only", "{stdout}");
}

#[test]
fn list_json_defaults_user_invocable_true() {
    let pkg = corpus().join("agentskills/minimal-valid");
    let (_home, mut cmd) = bin();
    let out = cmd
        .arg("list")
        .arg("--json")
        .arg("--path")
        .arg(&pkg)
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("list json");
    assert_eq!(v["skills"][0]["name"], "minimal-valid", "{stdout}");
    assert_eq!(
        v["skills"][0]["user_invocable"], true,
        "omitted user_invocable defaults true: {stdout}"
    );
    assert_eq!(
        v["skills"][0]["disable_model_invocation"], false,
        "omitted disable_model_invocation defaults false: {stdout}"
    );
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
