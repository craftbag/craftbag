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
fn list_help_names_vendor_path_examples() {
    let (_home, mut cmd) = bin();
    cmd.arg("list")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--vendor"))
        .stdout(predicates::str::contains("claude"))
        .stdout(predicates::str::contains("--path"))
        .stdout(predicates::str::contains("--format"))
        .stdout(predicates::str::contains("json, xml, catalog, watch"))
        .stdout(predicates::str::contains("watch-dirs"))
        .stdout(predicates::str::contains("Example:"));
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
        .stdout(predicates::str::contains("allowed-tools"))
        .stdout(predicates::str::contains("Example:"));
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
