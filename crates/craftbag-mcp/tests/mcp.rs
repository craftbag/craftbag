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
