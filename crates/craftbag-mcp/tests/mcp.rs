use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("craftbag-mcp").expect("bin")
}

fn corpus_pkg() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/agentskills/minimal-valid")
}

fn corpus_cursor_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/cursor-project")
}

fn corpus_grok_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/grok-project")
}

fn corpus_leftover_empty_nested_skills() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/leftover/empty-nested-skills")
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
