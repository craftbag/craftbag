use std::path::PathBuf;

use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("craftbag-mcp").expect("bin")
}

fn corpus_pkg() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/agentskills/minimal-valid")
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
