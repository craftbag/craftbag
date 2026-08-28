use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("craftbag-mcp").expect("bin")
}

#[test]
fn mcp_help_names_tools() {
    let out = bin()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("skills_list") && text.contains("skills_validate") && text.contains("stdio"),
        "craftbag-mcp --help must name tools and stdio: {text}"
    );
}

#[test]
fn mcp_version_prints_crate_version() {
    let out = bin()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "craftbag-mcp --version must print the crate version: {text}"
    );
}

fn corpus_pkg() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/agentskills/minimal-valid")
}

fn corpus_cursor_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/cursor-project")
}

fn corpus_claude_user() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/claude-user")
}

fn corpus_grok_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/grok-project")
}

fn corpus_bline_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/bline-project")
}

fn corpus_leftover_empty_nested_skills() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/leftover/empty-nested-skills")
}

fn corpus_leftover_skills_named_package() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/leftover/skills-named-package")
}

/// extra/skills/SKILL.md named loose next to extra/wanted. Tempfile
/// only; do not commit another leftover corpus tree.
fn leftover_extra_skills_loose() -> (tempfile::TempDir, PathBuf) {
    let extra = tempfile::tempdir().expect("extra");
    std::fs::create_dir_all(extra.path().join("skills")).expect("mkdir skills");
    std::fs::write(
        extra.path().join("skills").join("SKILL.md"),
        "---\nname: loose\ndescription: leftover\n---\nloose\n",
    )
    .expect("write leftover");
    let wanted = extra.path().join("wanted");
    std::fs::create_dir_all(&wanted).expect("mkdir wanted");
    let wanted_md = wanted.join("SKILL.md");
    std::fs::write(
        &wanted_md,
        "---\nname: wanted\ndescription: from-sibling\n---\nfrom-sibling\n",
    )
    .expect("write wanted");
    (extra, wanted_md)
}

fn corpus_vercel_npx() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/vercel-npx")
}

fn rpc(req: &serde_json::Value) -> serde_json::Value {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = tempfile::tempdir().expect("home");
    let out = bin()
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .write_stdin(format!("{req}\n"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().expect("one line");
    serde_json::from_str(line).unwrap_or_else(|e| panic!("json {e}: {line}"))
}

fn rpc_in(cwd: &Path, home: &Path, req: &serde_json::Value) -> serde_json::Value {
    let out = bin()
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .write_stdin(format!("{req}\n"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().expect("one line");
    serde_json::from_str(line).unwrap_or_else(|e| panic!("json {e}: {line}"))
}

#[test]
fn stdio_initialize() {
    let resp = rpc(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }));
    assert_eq!(resp["result"]["serverInfo"]["name"], "craftbag");
    assert_eq!(
        resp["result"]["capabilities"]["tools"],
        serde_json::json!({})
    );
}

#[test]
fn stdio_skills_validate_corpus() {
    let resp = rpc(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 80,
        "method": "tools/call",
        "params": {
            "name": "skills_validate",
            "arguments": {"path": corpus_pkg()}
        }
    }));
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("validate json");
    assert_eq!(v["ok"], true, "{text}");
    assert_eq!(v["name"], "minimal-valid", "{text}");
}

#[test]
fn stdio_skills_list_empty_path_does_not_scan_cwd() {
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let pkg = cwd.path().join("planted");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    std::fs::write(
        pkg.join("SKILL.md"),
        "---\nname: planted\ndescription: from-cwd\n---\nFROM_CWD\n",
    )
    .expect("write");
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "skills_list",
            "arguments": {"paths": [""]}
        }
    });
    let out = bin()
        .current_dir(cwd.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .write_stdin(format!("{req}\n"))
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().expect("one line");
    let resp: serde_json::Value =
        serde_json::from_str(line).unwrap_or_else(|e| panic!("json {e}: {line}"));
    assert_eq!(
        resp["result"]["isError"], false,
        "empty paths item must be ignored: {line}"
    );
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        !text.contains("planted") && !text.contains("FROM_CWD"),
        "empty extra-path must not load cwd package: {text}"
    );
}

#[test]
fn stdio_skills_list_corpus() {
    let resp = rpc(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "skills_list",
            "arguments": {"paths": [corpus_pkg()]}
        }
    }));
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(text.contains("minimal-valid"), "{text}");
}

#[test]
fn stdio_skills_list_catalog_and_xml_emit_skips() {
    // Sibling of list_catalog_and_xml_emit_skips_on_stderr. MCP stdio
    // has no stderr; catalog/xml used to return an empty prompt fragment
    // with no skip row, so a leftover-only tree looked like success.
    let tmp = tempfile::tempdir().expect("tmp");
    let skills = tmp.path().join(".agents").join("skills");
    std::fs::create_dir_all(&skills).expect("mkdir");
    std::fs::write(
        skills.join("SKILL.md"),
        "---\nname: loose\ndescription: leftover root file\n---\nbody\n",
    )
    .expect("write");
    let home = tempfile::tempdir().expect("home");
    for format in ["catalog", "xml"] {
        let resp = rpc_in(
            tmp.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 36,
                "method": "tools/call",
                "params": {
                    "name": "skills_list",
                    "arguments": {"format": format}
                }
            }),
        );
        assert_eq!(
            resp["result"]["isError"], false,
            "{format} leftover list is not a tool error: {resp}"
        );
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        assert!(
            !text.contains("- **loose**") && !text.contains("<name>loose</name>"),
            "{format} prompt fragment must not list the leftover as a skill: {text}"
        );
        assert!(
            text.contains("skip\troot_file"),
            "{format} must emit leftover skip TSV like CLI catalog stderr: {text:?}"
        );
    }
}

#[test]
fn stdio_skills_list_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the MCP door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus_leftover_empty_nested_skills();
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let row = skills
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("list must name wanted: {text}"));
    assert_eq!(
        row["source"], "extra",
        "list JSON source must be the wire token extra: {text}"
    );
    let names: Vec<&str> = skills.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(
        names,
        ["wanted"],
        "empty extra/skills must not hide leftover sibling packages: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize list path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "list path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(skips.is_empty(), "empty extra/skills is not a skip: {text}");
}

#[test]
fn stdio_skills_list_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the MCP door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus_leftover_skills_named_package();
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let mut names: Vec<&str> = skills.iter().filter_map(|s| s["name"].as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["skills", "wanted"],
        "named extra/skills must not hide leftover sibling or scan nested evil: {text}"
    );
    let wanted_row = skills
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("list must name wanted: {text}"));
    assert_eq!(
        wanted_row["source"], "extra",
        "list JSON source must be the wire token extra: {text}"
    );
    let path = wanted_row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize list path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "list path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "named extra/skills is not a leftover root file: {text}"
    );
}

#[test]
fn stdio_skills_list_leftover_skills_loose_md_names_wanted() {
    // Sibling lock of extra_path_skills_leftover_skill_md_does_not_hide_sibling
    // on the MCP list door. leftover extra/skills/SKILL.md named loose is
    // not exclusive-scan entries.
    let (extra, _wanted) = leftover_extra_skills_loose();
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "paths": [extra.path()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let names: Vec<&str> = skills.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(
        names,
        ["wanted"],
        "leftover extra/skills/SKILL.md must not hide leftover sibling packages: {text}"
    );
    let wanted_row = skills
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("list must name wanted: {text}"));
    assert_eq!(
        wanted_row["source"], "extra",
        "list JSON source must be the wire token extra: {text}"
    );
}

#[cfg(unix)]
fn mkfifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo {path:?} failed: {status}");
}

#[cfg(unix)]
#[test]
fn stdio_skills_list_leftover_skills_fifo_names_wanted() {
    // Sibling lock of extra_path_skills_fifo_skill_md_does_not_hide_sibling
    // on the MCP list door. FIFO extra/skills/SKILL.md is unreadable, not
    // exclusive-scan entries. Do not commit a FIFO in the corpus.
    let extra = tempfile::tempdir().expect("extra");
    let skills_dir = extra.path().join("skills");
    std::fs::create_dir_all(&skills_dir).expect("mkdir skills");
    let fifo = skills_dir.join("SKILL.md");
    mkfifo(&fifo);
    let wanted_pkg = extra.path().join("wanted");
    std::fs::create_dir_all(&wanted_pkg).expect("mkdir wanted");
    let wanted = wanted_pkg.join("SKILL.md");
    std::fs::write(
        &wanted,
        "---\nname: wanted\ndescription: from-sibling\n---\nfrom-sibling\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "paths": [extra.path()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let names: Vec<&str> = skills.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(
        names,
        ["wanted"],
        "FIFO extra/skills/SKILL.md must not hide leftover sibling packages: {text}"
    );
    let wanted_row = skills
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("list must name wanted: {text}"));
    assert_eq!(
        wanted_row["source"], "extra",
        "list JSON source must be the wire token extra: {text}"
    );
    let path = wanted_row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize list path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize wanted: {e}"));
    assert_eq!(got, want, "list path must be the wanted SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().any(|s| {
            s["kind"] == "unreadable"
                && Path::new(s["path"].as_str().unwrap_or("")) == fifo
                && s["detail"]
                    .as_str()
                    .is_some_and(|d| d.contains("regular file"))
        }),
        "FIFO extra/skills/SKILL.md must be unreadable: {text}"
    );
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "FIFO extra/skills/SKILL.md must not become a root_file peek: {text}"
    );
}

#[cfg(unix)]
#[test]
fn stdio_skills_list_newline_extra_path_does_not_name_demo() {
    // Sibling lock of package_path_with_newline_component_is_unreadable_not_loaded
    // on the MCP list door. Do not commit a newline path in the corpus.
    let parent = tempfile::tempdir().expect("parent");
    let extra = parent.path().join("evil\nroot");
    std::fs::create_dir_all(extra.join("demo")).expect("mkdir");
    std::fs::write(
        extra.join("demo").join("SKILL.md"),
        "---\nname: demo\ndescription: d\n---\nSECRET_BODY\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let names: Vec<&str> = v["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(
        !names.iter().any(|n| *n == "demo"),
        "must not name demo under newline extra-path: {text}"
    );
    assert!(
        !text.contains("SECRET_BODY"),
        "body must not appear in list: {text}"
    );
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().any(|s| {
            s["kind"] == "unreadable"
                && s["detail"].as_str().is_some_and(|d| {
                    d.contains("path") || d.contains("line") || d.contains("control")
                })
        }),
        "newline extra-path must be unreadable: {text}"
    );
    for s in skips {
        if let Some(p) = s["path"].as_str() {
            assert!(
                !p.contains('\n') && !p.contains('\u{2028}') && !p.contains('\u{2029}'),
                "skip path must not echo raw line separators: {p:?}"
            );
        }
        if let Some(d) = s["detail"].as_str() {
            assert!(
                !d.contains('\n') && !d.contains('\u{2028}') && !d.contains('\u{2029}'),
                "skip detail must not echo raw line separators: {d:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn stdio_skills_why_newline_extra_path_demo_is_unknown() {
    // Sibling lock of package_path_with_newline_component_is_unreadable_not_loaded
    // on the MCP why door. Do not commit a newline path in the corpus.
    let parent = tempfile::tempdir().expect("parent");
    let extra = parent.path().join("evil\nroot");
    std::fs::create_dir_all(extra.join("demo")).expect("mkdir");
    std::fs::write(
        extra.join("demo").join("SKILL.md"),
        "---\nname: demo\ndescription: d\n---\nSECRET_BODY\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "demo",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    let kind = resp["result"]["error_kind"].as_str().unwrap_or("");
    assert!(
        kind == "unknown_skill" || kind == "unreadable",
        "why miss must be unknown or unreadable: {resp}"
    );
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        !text.contains("SECRET_BODY"),
        "body must not load via why: {text}"
    );
    assert_eq!(
        text.lines().count(),
        1,
        "why miss must stay one line: {text:?}"
    );
    assert!(
        !text.contains('\n') && !text.contains('\u{2028}') && !text.contains('\u{2029}'),
        "why miss must not echo raw line separators: {text:?}"
    );
    for key in ["error", "path", "detail"] {
        if let Some(s) = resp["result"].get(key).and_then(|x| x.as_str()) {
            assert!(
                !s.contains('\n') && !s.contains('\u{2028}') && !s.contains('\u{2029}'),
                "miss {key} must not echo raw line separators: {s:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn stdio_skills_list_watch_newline_extra_path_omits_root() {
    // Sibling lock of package_path_with_newline_component_is_unreadable_not_loaded
    // on the MCP watch door. Do not commit a newline path in the corpus.
    let parent = tempfile::tempdir().expect("parent");
    let extra = parent.path().join("evil\nroot");
    std::fs::create_dir_all(extra.join("demo")).expect("mkdir");
    std::fs::write(
        extra.join("demo").join("SKILL.md"),
        "---\nname: demo\ndescription: d\n---\nSECRET_BODY\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "format": "watch",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        !text.contains("evil\nroot") && !text.contains('\u{2028}') && !text.contains('\u{2029}'),
        "watch must not echo a raw newline path: {text:?}"
    );
    assert!(
        text.lines().all(|l| Path::new(l) != extra.as_path()),
        "must not watch newline extra-path root: {text}"
    );
    assert!(
        !text.contains("SECRET_BODY") && !text.contains("demo") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}

#[test]
fn stdio_skills_list_watch_whitespace_extra_path_omits_root() {
    // Sibling lock of extra_path_whitespace_dotdot_does_not_scan_filesystem_root
    // on the MCP watch door. paths: [" /.."] and ["/ .."] must not
    // notify-watch `/`.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    for raw in [
        " /..",
        "\t/..",
        "\u{85}/..",
        "\u{00a0}/..",
        "/ ..",
        "/\t..",
        "/\u{00a0}..",
    ] {
        let resp = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 20,
                "method": "tools/call",
                "params": {
                    "name": "skills_list",
                    "arguments": {
                        "format": "watch",
                        "paths": [raw],
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(resp["result"]["isError"], false, "raw={raw:?} {resp}");
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        assert!(
            text.lines().all(|l| {
                let p = Path::new(l);
                p != Path::new("/") && p != Path::new("/..")
            }),
            "watch must not list a collapsed extra-path token {raw:?}: {text:?}"
        );
        assert!(
            text.trim().is_empty(),
            "collapsed extra-path {raw:?} must not add a watch root: {text:?}"
        );
    }
}

#[test]
fn stdio_skills_list_whitespace_user_dir_does_not_scan_root() {
    // Sibling lock of extra_path_whitespace_dotdot_does_not_scan_filesystem_root
    // on the MCP user_dir list door. user_dir: " /.." and "/ .." must not
    // rewrite to `/`.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    for raw in [
        " /..",
        "\t/..",
        "\u{85}/..",
        "\u{00a0}/..",
        "/ ..",
        "/\t..",
        "/\u{00a0}..",
    ] {
        let resp = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 42,
                "method": "tools/call",
                "params": {
                    "name": "skills_list",
                    "arguments": {
                        "user_dir": raw,
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(resp["result"]["isError"], false, "raw={raw:?} {resp}");
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        let v: serde_json::Value = serde_json::from_str(text).expect("list json");
        let skills = v["skills"].as_array().expect("skills");
        assert!(
            skills.is_empty(),
            "user_dir {raw:?} must not load root packages: {text}"
        );
        let skips = v["skips"].as_array().expect("skips");
        assert!(
            skips.iter().any(|s| {
                s["kind"] == "unreadable"
                    && s["detail"]
                        .as_str()
                        .is_some_and(|d| d.contains("collapses"))
            }),
            "user_dir {raw:?} must skip as collapse, not walk /: {text}"
        );
    }
}

#[test]
fn stdio_skills_list_watch_whitespace_user_dir_omits_root() {
    // Sibling lock of extra_path_whitespace_dotdot_does_not_scan_filesystem_root
    // on the MCP user_dir watch door. user_dir: " /.." and "/ .." must not
    // notify-watch `/`.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    for raw in [
        " /..",
        "\t/..",
        "\u{85}/..",
        "\u{00a0}/..",
        "/ ..",
        "/\t..",
        "/\u{00a0}..",
    ] {
        let resp = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 43,
                "method": "tools/call",
                "params": {
                    "name": "skills_list",
                    "arguments": {
                        "format": "watch",
                        "user_dir": raw,
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(resp["result"]["isError"], false, "raw={raw:?} {resp}");
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        assert!(
            text.lines().all(|l| {
                let p = Path::new(l);
                p != Path::new("/") && p != Path::new("/..")
            }),
            "watch must not list a collapsed user_dir token {raw:?}: {text:?}"
        );
        assert!(
            text.trim().is_empty(),
            "collapsed user_dir {raw:?} must not add a watch root: {text:?}"
        );
    }
}

#[test]
fn stdio_skills_why_whitespace_extra_path_demo_is_unknown() {
    // Sibling lock of extra_path_whitespace_dotdot_does_not_scan_filesystem_root
    // on the MCP why door. paths: [" /.."] and ["/ .."] must not rewrite to `/`.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    for raw in [
        " /..",
        "\t/..",
        "\u{85}/..",
        "\u{00a0}/..",
        "/ ..",
        "/\t..",
        "/\u{00a0}..",
    ] {
        let named = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 21,
                "method": "tools/call",
                "params": {
                    "name": "skills_why",
                    "arguments": {
                        "name": "demo",
                        "paths": [raw],
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(
            named["result"]["isError"], true,
            "why demo via extra-path {raw:?} must miss: {named}"
        );
        let kind = named["result"]["error_kind"].as_str().unwrap_or("");
        assert!(
            kind == "unknown_skill" || kind == "unreadable",
            "why miss must be unknown or unreadable for {raw:?}: {named}"
        );
        let named_text = named["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert_eq!(
            named_text.lines().count(),
            1,
            "why miss must stay one line: {named_text:?}"
        );
        assert!(
            !named_text.contains("[Activated skill:"),
            "why must not activate via extra-path {raw:?}: {named_text}"
        );
        if let Some(p) = named["result"].get("path").and_then(|x| x.as_str()) {
            assert_ne!(p, "/", "must not peel path=/ for {raw:?}: {named}");
            assert_ne!(
                p, "/SKILL.md",
                "must not peel path=/SKILL.md for {raw:?}: {named}"
            );
        }

        let all = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 22,
                "method": "tools/call",
                "params": {
                    "name": "skills_why",
                    "arguments": {
                        "paths": [raw],
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(
            all["result"]["isError"], false,
            "unfiltered why via extra-path {raw:?} must stay ok: {all}"
        );
        let all_text = all["result"]["content"][0]["text"].as_str().expect("text");
        let v: serde_json::Value = serde_json::from_str(all_text).expect("why json");
        let loaded = v["loaded"].as_array().expect("loaded");
        assert!(
            loaded.is_empty(),
            "unfiltered why must not load root via extra-path {raw:?}: {all_text}"
        );
        let skips = v["skips"].as_array().expect("skips");
        assert!(
            skips.iter().any(|s| {
                s["kind"] == "unreadable"
                    && s["detail"]
                        .as_str()
                        .is_some_and(|d| d.contains("collapses"))
            }),
            "why must keep the collapse skip for extra-path {raw:?}: {all_text}"
        );
    }
}

#[test]
fn stdio_skills_load_whitespace_extra_path_demo_is_unknown() {
    // Sibling lock of extra_path_whitespace_dotdot_does_not_scan_filesystem_root
    // on the MCP load door. paths: [" /.."] and ["/ .."] must not rewrite to `/`.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    for raw in [
        " /..",
        "\t/..",
        "\u{85}/..",
        "\u{00a0}/..",
        "/ ..",
        "/\t..",
        "/\u{00a0}..",
    ] {
        let resp = rpc_in(
            cwd.path(),
            home.path(),
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 23,
                "method": "tools/call",
                "params": {
                    "name": "skills_load",
                    "arguments": {
                        "name": "demo",
                        "paths": [raw],
                        "implicit_roots": false
                    }
                }
            }),
        );
        assert_eq!(
            resp["result"]["isError"], true,
            "load demo via extra-path {raw:?} must miss: {resp}"
        );
        let kind = resp["result"]["error_kind"].as_str().unwrap_or("");
        assert!(
            kind == "unknown_skill" || kind == "unreadable",
            "load miss must be unknown or unreadable for {raw:?}: {resp}"
        );
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        assert_eq!(
            text.lines().count(),
            1,
            "load miss must stay one line: {text:?}"
        );
        assert!(
            !text.contains("[Activated skill:"),
            "must not activate demo via extra-path {raw:?}: {text}"
        );
        if let Some(p) = resp["result"].get("path").and_then(|x| x.as_str()) {
            assert_ne!(p, "/", "must not peel path=/ for {raw:?}: {resp}");
            assert_ne!(
                p, "/SKILL.md",
                "must not peel path=/SKILL.md for {raw:?}: {resp}"
            );
        }
    }
}

#[test]
fn stdio_load_why_collapse_refuse_peels_unreadable_and_names_field() {
    // Sibling of CLI load_why_collapse_refuse_peels_unreadable_and_names_flag.
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let raw = " /..";

    let load_path = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 24,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "demo",
                    "paths": [raw],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(load_path["result"]["isError"], true, "{load_path}");
    assert_eq!(
        load_path["result"]["error_kind"], "unreadable",
        "MCP load paths collapse must peel unreadable: {load_path}"
    );
    assert!(
        load_path["result"]
            .get("path")
            .and_then(|p| p.as_str())
            .is_some(),
        "MCP load must peel path: {load_path}"
    );
    let load_text = load_path["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        load_text.contains("--path") && load_text.contains("paths"),
        "MCP load error must name --path / paths: {load_text}"
    );

    let why_path = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 25,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "demo",
                    "paths": [raw],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(why_path["result"]["isError"], true, "{why_path}");
    assert_eq!(
        why_path["result"]["error_kind"], "unreadable",
        "MCP why paths collapse must peel unreadable: {why_path}"
    );
    let why_text = why_path["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        why_text.contains("--path"),
        "MCP why error must name --path: {why_text}"
    );

    let load_user = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 26,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "demo",
                    "user_dir": raw,
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(load_user["result"]["isError"], true, "{load_user}");
    assert_eq!(
        load_user["result"]["error_kind"], "unreadable",
        "MCP load user_dir collapse must peel unreadable: {load_user}"
    );
    let user_text = load_user["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        user_text.contains("--user-dir") && user_text.contains("user_dir"),
        "MCP load error must name --user-dir / user_dir: {user_text}"
    );
}

#[cfg(unix)]
#[test]
fn stdio_skills_load_newline_extra_path_demo_is_unknown() {
    // Sibling lock of package_path_with_newline_component_is_unreadable_not_loaded
    // on the MCP load door. Do not commit a newline path in the corpus.
    let parent = tempfile::tempdir().expect("parent");
    let extra = parent.path().join("evil\nroot");
    std::fs::create_dir_all(extra.join("demo")).expect("mkdir");
    std::fs::write(
        extra.join("demo").join("SKILL.md"),
        "---\nname: demo\ndescription: d\n---\nSECRET_BODY\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "demo",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    let kind = resp["result"]["error_kind"].as_str().unwrap_or("");
    assert!(
        kind == "unknown_skill" || kind == "unreadable",
        "load miss must be unknown or unreadable: {resp}"
    );
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        !text.contains("SECRET_BODY"),
        "body must not load via skills_load: {text}"
    );
    assert_eq!(
        text.lines().count(),
        1,
        "load miss must stay one line: {text:?}"
    );
    assert!(
        !text.contains('\n') && !text.contains('\u{2028}') && !text.contains('\u{2029}'),
        "load miss must not echo raw line separators: {text:?}"
    );
    for key in ["error", "path", "detail"] {
        if let Some(s) = resp["result"].get(key).and_then(|x| x.as_str()) {
            assert!(
                !s.contains('\n') && !s.contains('\u{2028}') && !s.contains('\u{2029}'),
                "miss {key} must not echo raw line separators: {s:?}"
            );
        }
    }
}

#[test]
fn stdio_skills_why_text_leftover_hostile_loaded_tsv_stays_one_row() {
    // CLI why (not --json) uses format_why_text (PR 282). MCP skills_why
    // was JSON-only, so leftover implicit paths stayed raw in the tool
    // text. format=text must share those sanitized TSV rows.
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("evil\u{2028}root");
    let pkg = root.join(".agents").join("skills").join("demo");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    std::fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: leftover\u{2014}pkg\n---\nbody\n",
    )
    .expect("write");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        &root,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 80,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "format": "text"
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let loaded: Vec<&str> = text.lines().filter(|l| l.starts_with("loaded\t")).collect();
    assert_eq!(
        loaded.len(),
        1,
        "MCP why text leftover loaded TSV must stay one row: {text:?}"
    );
    let row = loaded[0];
    assert!(
        row.starts_with("loaded\tdemo\t"),
        "MCP why format=text must emit loaded TSV like CLI why: {row:?}"
    );
    assert!(
        !row.contains('\u{2028}') && !row.contains('\u{2014}'),
        "U+2028 / em dash must not leak into MCP why text: {row:?}"
    );
    assert!(
        row.contains("evil?root"),
        "hostile leftover path must be sanitized on MCP why text: {row:?}"
    );
}

#[test]
fn stdio_skills_load_leftover_hostile_path_stays_one_envelope_line() {
    // CLI load envelope leftover path is the sibling of leftover loaded
    // TSV (PR 282). MCP skills_load shares format_load_message, so the
    // same implicit package must sanitize Skill package root (evil?root).
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("evil\u{2028}root");
    let pkg = root.join(".agents").join("skills").join("demo");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    std::fs::write(
        pkg.join("SKILL.md"),
        "---\nname: demo\ndescription: leftover\u{2014}pkg\n---\nbody\n",
    )
    .expect("write");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        &root,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 81,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": { "name": "demo" }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let header = text.split("\n---\n").next().expect("header");
    let root_line = header
        .lines()
        .find(|l| l.starts_with("Skill package root:"))
        .expect("root line");
    assert_eq!(
        header
            .lines()
            .filter(|l| l.starts_with("Skill package root:"))
            .count(),
        1,
        "MCP load leftover envelope root must stay one line: {header}"
    );
    assert!(
        !root_line.contains('\u{2028}') && !root_line.contains('\u{2014}'),
        "U+2028 / em dash must not leak into MCP load envelope path: {root_line}"
    );
    assert!(
        root_line.contains("evil?root"),
        "hostile leftover path must be sanitized on MCP load envelope: {root_line}"
    );
}

#[test]
fn stdio_skills_why_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the MCP why door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus_leftover_empty_nested_skills();
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {text}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(skips.is_empty(), "empty extra/skills is not a skip: {text}");
}

#[test]
fn stdio_skills_why_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the MCP why door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus_leftover_skills_named_package();
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {text}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "named extra/skills is not a leftover root file: {text}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "skills",
                    "paths": [extra.clone()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    assert!(
        loaded.iter().any(|s| s["name"] == "skills"),
        "why must name the extra/skills package: {text}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "evil",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    assert_eq!(
        resp["result"]["error_kind"], "unknown_skill",
        "nested evil must stay unknown: {resp}"
    );
}

#[test]
fn stdio_skills_why_leftover_skills_loose_md_names_wanted() {
    // Sibling lock of extra_path_skills_leftover_skill_md_does_not_hide_sibling
    // on the MCP why door.
    let (extra, wanted) = leftover_extra_skills_loose();
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra.path()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {text}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
}

#[cfg(unix)]
#[test]
fn stdio_skills_why_leftover_skills_fifo_names_wanted() {
    // Sibling lock of extra_path_skills_fifo_skill_md_does_not_hide_sibling
    // on the MCP why door. FIFO extra/skills/SKILL.md is unreadable and must
    // not hide leftover extra/wanted. Do not commit a FIFO in the corpus.
    let extra = tempfile::tempdir().expect("extra");
    let skills_dir = extra.path().join("skills");
    std::fs::create_dir_all(&skills_dir).expect("mkdir skills");
    mkfifo(&skills_dir.join("SKILL.md"));
    let wanted_pkg = extra.path().join("wanted");
    std::fs::create_dir_all(&wanted_pkg).expect("mkdir wanted");
    let wanted = wanted_pkg.join("SKILL.md");
    std::fs::write(
        &wanted,
        "---
name: wanted
description: from-sibling
---
from-sibling
",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra.path()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "wanted")
        .unwrap_or_else(|| panic!("why must name wanted: {text}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = wanted
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize wanted: {e}"));
    assert_eq!(got, want, "why path must be the wanted SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.iter().all(|s| s["kind"] != "root_file"),
        "FIFO extra/skills/SKILL.md must not become a root_file peek: {text}"
    );
}

#[test]
fn stdio_skills_load_leftover_empty_nested_skills_names_wanted() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the MCP load door. Default vendor stays off. Empty extra/skills
    // must not hide extra/wanted.
    let extra = corpus_leftover_empty_nested_skills();
    let skill = extra.join("wanted/SKILL.md");
    assert!(
        skill.is_file(),
        "committed leftover empty extra/skills fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: wanted]"),
        "empty extra/skills must not hide leftover sibling wanted: {text}"
    );
    assert!(
        text.contains("from-sibling"),
        "must load leftover sibling body: {text}"
    );
}

#[test]
fn stdio_skills_load_leftover_skills_named_package_names_wanted() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the MCP load door. extra/skills/SKILL.md named skills is a package.
    let extra = corpus_leftover_skills_named_package();
    let wanted = extra.join("wanted/SKILL.md");
    let skills_md = extra.join("skills/SKILL.md");
    assert!(
        wanted.is_file() && skills_md.is_file(),
        "committed leftover named extra/skills fixture must exist: {}",
        extra.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra.clone()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: wanted]"),
        "named extra/skills must not hide leftover sibling wanted: {text}"
    );
    assert!(
        text.contains("from-sibling"),
        "must load leftover sibling body: {text}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "skills",
                    "paths": [extra.clone()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: skills]"),
        "named extra/skills package must load: {text}"
    );
    assert!(
        text.contains("PACKAGE_BODY"),
        "must load named extra/skills body: {text}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "evil",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    assert_eq!(
        resp["result"]["error_kind"], "unknown_skill",
        "nested evil must stay unknown: {resp}"
    );
}

#[test]
fn stdio_skills_load_leftover_skills_loose_md_names_wanted() {
    // Sibling lock of extra_path_skills_leftover_skill_md_does_not_hide_sibling
    // on the MCP load door.
    let (extra, _wanted) = leftover_extra_skills_loose();
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra.path()],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: wanted]"),
        "leftover extra/skills/SKILL.md must not hide leftover sibling wanted: {text}"
    );
    assert!(
        text.contains("from-sibling"),
        "must load leftover sibling body: {text}"
    );
}

#[cfg(unix)]
#[test]
fn stdio_skills_load_leftover_skills_fifo_names_wanted() {
    // Sibling lock of extra_path_skills_fifo_skill_md_does_not_hide_sibling
    // on the MCP load door. FIFO extra/skills/SKILL.md is unreadable, not
    // exclusive-scan entries. Do not commit a FIFO in the corpus.
    let extra = tempfile::tempdir().expect("extra");
    let skills_dir = extra.path().join("skills");
    std::fs::create_dir_all(&skills_dir).expect("mkdir skills");
    mkfifo(&skills_dir.join("SKILL.md"));
    let wanted_pkg = extra.path().join("wanted");
    std::fs::create_dir_all(&wanted_pkg).expect("mkdir wanted");
    std::fs::write(
        wanted_pkg.join("SKILL.md"),
        "---\nname: wanted\ndescription: from-sibling\n---\nfrom-sibling\n",
    )
    .expect("write");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let extra_path = extra.path();
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "wanted",
                    "paths": [extra_path],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: wanted]"),
        "FIFO extra/skills/SKILL.md must not hide leftover sibling wanted: {text}"
    );
    assert!(
        text.contains("from-sibling"),
        "must load leftover sibling body: {text}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 23,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "skills",
                    "paths": [extra_path],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    assert!(
        matches!(
            resp["result"]["error_kind"].as_str(),
            Some("unknown_skill") | Some("unreadable")
        ),
        "FIFO extra/skills must not load (unknown or skip): {resp}"
    );
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 24,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "evil",
                    "paths": [extra_path],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], true, "{resp}");
    assert_eq!(
        resp["result"]["error_kind"], "unknown_skill",
        "nested evil must stay unknown: {resp}"
    );
}

#[test]
fn stdio_skills_why_incumbent_vercel_npx_names_deploy_hint() {
    // Sibling lock of incumbent_vercel_skills_dir_as_extra_path
    // on the MCP why door. Isolated HOME; default vendor stays off.
    let extra = corpus_vercel_npx();
    let skill = extra.join("skills/deploy-hint/SKILL.md");
    assert!(
        skill.is_file(),
        "committed vercel-npx extra-path fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 25,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {
                    "name": "deploy-hint",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "deploy-hint")
        .unwrap_or_else(|| panic!("why must name deploy-hint: {text}"));
    assert_eq!(
        row["source"], "extra",
        "why JSON source must be the wire token extra: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "vercel-npx extra/skills is not a skip: {text}"
    );
}

#[test]
fn stdio_skills_load_incumbent_vercel_npx_names_deploy_hint() {
    // Sibling lock of incumbent_vercel_skills_dir_as_extra_path
    // on the MCP load door. Isolated HOME; default vendor stays off.
    let extra = corpus_vercel_npx();
    let skill = extra.join("skills/deploy-hint/SKILL.md");
    assert!(
        skill.is_file(),
        "committed vercel-npx extra-path fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 26,
            "method": "tools/call",
            "params": {
                "name": "skills_load",
                "arguments": {
                    "name": "deploy-hint",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.contains("[Activated skill: deploy-hint]"),
        "vercel-npx extra-path must load deploy-hint: {text}"
    );
    assert!(
        text.contains("Read this when deploying."),
        "must load deploy-hint body: {text}"
    );
}

#[test]
fn stdio_skills_why_vendor_cursor_names_create_rule() {
    let cwd = corpus_cursor_project();
    let skill = cwd.join(".cursor/skills/create-rule/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Cursor project fixture must exist: {}",
        skill.display()
    );
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "create-rule"}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], true,
        "cursor vendor is opt-in: {off}"
    );
    assert_eq!(
        off["result"]["error_kind"], "unknown_skill",
        "why without vendor cursor must peel unknown_skill: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    assert!(
        off_text.contains("unknown skill: create-rule"),
        "why without vendor cursor must not see create-rule: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "create-rule", "vendor": ["cursor"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "create-rule")
        .unwrap_or_else(|| panic!("why must name create-rule: {text}"));
    assert_eq!(
        row["source"], "cursor",
        "why JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "create-rule is a loaded vendor skill, not a skip: {text}"
    );
}

#[test]
fn stdio_skills_list_vendor_cursor_names_create_rule() {
    let cwd = corpus_cursor_project();
    let skill = cwd.join(".cursor/skills/create-rule/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Cursor project fixture must exist: {}",
        skill.display()
    );
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], false,
        "list without vendor is not a miss: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    let off_v: serde_json::Value = serde_json::from_str(off_text).expect("list json");
    let off_skills = off_v["skills"].as_array().expect("skills");
    assert!(
        off_skills.iter().all(|s| s["name"] != "create-rule"),
        "cursor vendor is opt-in: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {"vendor": ["cursor"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let row = skills
        .iter()
        .find(|s| s["name"] == "create-rule")
        .unwrap_or_else(|| panic!("list must name create-rule: {text}"));
    assert_eq!(
        row["source"], "cursor",
        "list JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize list path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "list path must be the fixture SKILL.md: {text}");
}

#[test]
fn stdio_skills_list_vendor_grok_names_project_grok() {
    let cwd = corpus_grok_project();
    let skill = cwd.join(".grok/skills/project-grok/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Grok project fixture must exist: {}",
        skill.display()
    );
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], false,
        "list without vendor is not a miss: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    let off_v: serde_json::Value = serde_json::from_str(off_text).expect("list json");
    let off_skills = off_v["skills"].as_array().expect("skills");
    assert!(
        off_skills.iter().all(|s| s["name"] != "project-grok"),
        "grok vendor is opt-in: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {"vendor": ["grok"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("list json");
    let skills = v["skills"].as_array().expect("skills");
    let row = skills
        .iter()
        .find(|s| s["name"] == "project-grok")
        .unwrap_or_else(|| panic!("list must name project-grok: {text}"));
    assert_eq!(
        row["source"], "grok",
        "list JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize list path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "list path must be the fixture SKILL.md: {text}");
}

#[test]
fn stdio_skills_why_vendor_grok_names_project_grok() {
    let cwd = corpus_grok_project();
    let skill = cwd.join(".grok/skills/project-grok/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Grok project fixture must exist: {}",
        skill.display()
    );
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "project-grok"}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], true,
        "grok vendor is opt-in: {off}"
    );
    assert_eq!(
        off["result"]["error_kind"], "unknown_skill",
        "why without vendor grok must peel unknown_skill: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    assert!(
        off_text.contains("unknown skill: project-grok"),
        "why without vendor grok must not see project-grok: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "project-grok", "vendor": ["grok"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "project-grok")
        .unwrap_or_else(|| panic!("why must name project-grok: {text}"));
    assert_eq!(
        row["source"], "grok",
        "why JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "project-grok is a loaded vendor skill, not a skip: {text}"
    );
}

#[test]
fn stdio_skills_why_vendor_bline_names_project_bline() {
    // Sibling lock of why_vendor_bline_names_project_bline on the MCP
    // why door. Isolated HOME; default vendor stays off.
    let cwd = corpus_bline_project();
    let skill = cwd.join(".bline/skills/project-bline/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Bline project fixture must exist: {}",
        skill.display()
    );
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "project-bline"}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], true,
        "bline vendor is opt-in: {off}"
    );
    assert_eq!(
        off["result"]["error_kind"], "unknown_skill",
        "why without vendor bline must peel unknown_skill: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    assert!(
        off_text.contains("unknown skill: project-bline"),
        "why without vendor bline must not see project-bline: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "project-bline", "vendor": ["bline"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "project-bline")
        .unwrap_or_else(|| panic!("why must name project-bline: {text}"));
    assert_eq!(
        row["source"], "bline",
        "why JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "project-bline is a loaded vendor skill, not a skip: {text}"
    );
}

#[test]
fn stdio_skills_why_vendor_claude_names_home_note() {
    // Sibling lock of incumbent_claude_user_home_vendor_layout_loads
    // on the MCP why door. Isolated cwd; HOME is the committed fixture.
    let home = corpus_claude_user();
    let skill = home.join(".claude/skills/home-note/SKILL.md");
    assert!(
        skill.is_file(),
        "committed Claude user-home fixture must exist: {}",
        skill.display()
    );
    let cwd = tempfile::tempdir().expect("cwd");
    let off = rpc_in(
        cwd.path(),
        &home,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "home-note"}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], true,
        "claude vendor is opt-in: {off}"
    );
    assert_eq!(
        off["result"]["error_kind"], "unknown_skill",
        "why without vendor claude must peel unknown_skill: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    assert!(
        off_text.contains("unknown skill: home-note"),
        "why without vendor claude must not see home-note: {off_text}"
    );

    let on = rpc_in(
        cwd.path(),
        &home,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "skills_why",
                "arguments": {"name": "home-note", "vendor": ["claude"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    let v: serde_json::Value = serde_json::from_str(text).expect("why json");
    let loaded = v["loaded"].as_array().expect("loaded");
    let row = loaded
        .iter()
        .find(|s| s["name"] == "home-note")
        .unwrap_or_else(|| panic!("why must name home-note: {text}"));
    assert_eq!(
        row["source"], "claude",
        "why JSON source must be the wire token: {text}"
    );
    let path = row["path"].as_str().expect("path");
    let got = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize why path {path}: {e}"));
    let want = skill
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize fixture: {e}"));
    assert_eq!(got, want, "why path must be the fixture SKILL.md: {text}");
    let skips = v["skips"].as_array().expect("skips");
    assert!(
        skips.is_empty(),
        "home-note is a loaded vendor skill, not a skip: {text}"
    );
}

#[test]
fn stdio_skills_list_watch_vendor_grok_lists_project_skills() {
    let cwd = corpus_grok_project();
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
    let home = tempfile::tempdir().expect("home");
    let off = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {"format": "watch"}
            }
        }),
    );
    assert_eq!(
        off["result"]["isError"], false,
        "watch without vendor is not a miss: {off}"
    );
    let off_text = off["result"]["content"][0]["text"]
        .as_str()
        .expect("off text");
    assert!(
        off_text.lines().all(|l| Path::new(l) != want.as_path()),
        "watch without vendor grok must not list .grok/skills: {off_text}"
    );

    let on = rpc_in(
        &cwd,
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {"format": "watch", "vendor": ["grok"]}
            }
        }),
    );
    assert_eq!(on["result"]["isError"], false, "{on}");
    let text = on["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.lines().any(|l| Path::new(l) == want.as_path()),
        "watch --vendor grok must list .grok/skills: {text}"
    );
    assert!(
        !text.contains("project-grok") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}

#[test]
fn stdio_skills_list_watch_leftover_empty_nested_skills_lists_extra_root() {
    // Sibling lock of extra_path_empty_skills_subdir_does_not_hide_sibling
    // on the MCP watch door. Default vendor stays off. Empty extra/skills
    // is not a discover walk.
    let extra = corpus_leftover_empty_nested_skills()
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
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "format": "watch",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.lines().any(|l| Path::new(l) == extra.as_path()),
        "watch must list leftover extra-path root: {text}"
    );
    assert!(
        text.lines().all(|l| Path::new(l) != extra_skills.as_path()),
        "empty extra/skills is not a discover walk: {text}"
    );
    assert!(
        !text.contains("wanted") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}

#[test]
fn stdio_skills_list_watch_leftover_skills_named_package_lists_extra_root() {
    // Sibling lock of extra_path_skills_named_package_does_not_hide_sibling
    // on the MCP watch door. Default vendor stays off. Named extra/skills
    // is a package, not a discover walk.
    let extra = corpus_leftover_skills_named_package()
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
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 17,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "format": "watch",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.lines().any(|l| Path::new(l) == extra.as_path()),
        "watch must list leftover extra-path root: {text}"
    );
    assert!(
        text.lines().all(|l| Path::new(l) != extra_skills.as_path()),
        "named extra/skills is not a discover walk: {text}"
    );
    assert!(
        !text.contains("wanted") && !text.contains("evil") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}

#[test]
fn stdio_skills_list_watch_leftover_skills_loose_md_lists_extra_root() {
    // Sibling lock of extra_path_skills_leftover_skill_md_does_not_hide_sibling
    // on the MCP watch door. leftover extra/skills/SKILL.md is not a
    // discover walk.
    let (extra_td, _wanted) = leftover_extra_skills_loose();
    let extra = extra_td
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize leftover extra: {e}"));
    let extra_skills = extra.join("skills");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "format": "watch",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.lines().any(|l| Path::new(l) == extra.as_path()),
        "watch format must list leftover extra-path root: {text}"
    );
    assert!(
        text.lines().all(|l| Path::new(l) != extra_skills.as_path()),
        "leftover extra/skills/SKILL.md is not a discover walk: {text}"
    );
    assert!(
        !text.contains("wanted") && !text.contains("loose") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}

#[cfg(unix)]
#[test]
fn stdio_skills_list_watch_leftover_skills_fifo_lists_extra_root() {
    // Sibling lock of extra_path_skills_fifo_skill_md_does_not_hide_sibling
    // on the MCP watch door. Default vendor stays off. FIFO extra/skills/SKILL.md
    // is unreadable; watch lists the extra root, not extra/skills. Do not commit
    // a FIFO in the corpus.
    let extra_td = tempfile::tempdir().expect("extra");
    let skills_dir = extra_td.path().join("skills");
    std::fs::create_dir_all(&skills_dir).expect("mkdir skills");
    mkfifo(&skills_dir.join("SKILL.md"));
    let wanted_pkg = extra_td.path().join("wanted");
    std::fs::create_dir_all(&wanted_pkg).expect("mkdir wanted");
    std::fs::write(
        wanted_pkg.join("SKILL.md"),
        "---\nname: wanted\ndescription: from-sibling\n---\nfrom-sibling\n",
    )
    .expect("write");
    let extra = extra_td
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize leftover extra: {e}"));
    let extra_skills = extra.join("skills");
    let cwd = tempfile::tempdir().expect("cwd");
    let home = tempfile::tempdir().expect("home");
    let resp = rpc_in(
        cwd.path(),
        home.path(),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 18,
            "method": "tools/call",
            "params": {
                "name": "skills_list",
                "arguments": {
                    "format": "watch",
                    "paths": [extra],
                    "implicit_roots": false
                }
            }
        }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().expect("text");
    assert!(
        text.lines().any(|l| Path::new(l) == extra.as_path()),
        "watch must list leftover extra-path root: {text}"
    );
    assert!(
        text.lines().all(|l| Path::new(l) != extra_skills.as_path()),
        "FIFO extra/skills is not a discover walk: {text}"
    );
    assert!(
        !text.contains("wanted") && !text.contains("## Skills"),
        "watch format must not load SKILL.md: {text}"
    );
}
