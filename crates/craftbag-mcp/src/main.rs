//! MCP stdio server: `skills_list`, `skills_load`, `skills_why`.
//!
//! Official `rmcp` 1.0+ uses let-chains and does not compile on MSRV 1.85.
//! This binary speaks MCP JSON-RPC over stdio and wraps the same library
//! the CLI uses.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use craftbag::{
    DiscoveryOptions, FormatOptions, SkillSource, discover, find_skill_by_name,
    format_available_skills_xml, format_load_message, progressive_budgets,
    unknown_or_skipped_skill_message, why,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

/// Present JSON `null` is a type error. Missing fields still default.
fn present_non_null<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Default, Deserialize)]
struct DiscoverArgs {
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    vendor: Vec<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    user_dir: Option<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    format: Option<String>,
    /// Reject names outside `a-z0-9-`. Omitted is false (Unicode / NFKC).
    #[serde(default)]
    ascii_names: bool,
}

#[derive(Debug, Deserialize)]
struct LoadArgs {
    name: String,
    #[serde(default, deserialize_with = "present_non_null")]
    args: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    vendor: Vec<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    user_dir: Option<String>,
    /// Reject names outside `a-z0-9-`. Omitted is false (Unicode / NFKC).
    #[serde(default)]
    ascii_names: bool,
}

#[derive(Debug, Default, Deserialize)]
struct WhyArgs {
    #[serde(default, deserialize_with = "present_non_null")]
    name: Option<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    context: Option<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    context_tokens: Option<usize>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    vendor: Vec<String>,
    #[serde(default, deserialize_with = "present_non_null")]
    user_dir: Option<String>,
    /// Reject names outside `a-z0-9-`. Omitted is false (Unicode / NFKC).
    #[serde(default)]
    ascii_names: bool,
}

#[derive(Debug, Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

fn opts_from(
    paths: Vec<String>,
    vendor: Vec<String>,
    user_dir: Option<String>,
    ascii_names: bool,
) -> Result<DiscoveryOptions, String> {
    // CLI clap rejects `--user-dir` with no value. Present empty or
    // whitespace is the same miss, not discover-cwd as User.
    if let Some(d) = user_dir.as_deref() {
        if d.trim().is_empty() {
            return Err("user_dir must be a non-empty string".to_owned());
        }
    }
    Ok(DiscoveryOptions {
        paths,
        vendor_roots: SkillSource::parse_vendor_roots(vendor)?,
        user_skills_dir: user_dir.map(PathBuf::from),
        ascii_names,
        ..DiscoveryOptions::default()
    })
}

fn list_json(args: DiscoverArgs) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let report = discover(
        &cwd,
        &opts_from(args.paths, args.vendor, args.user_dir, args.ascii_names)?,
    )
    .map_err(|e| e.to_string())?;
    let format = args.format.as_deref().unwrap_or("json");
    if format == "xml" {
        return Ok(format_available_skills_xml(&report.skills));
    }
    if format != "json" {
        return Err(format!("unknown format: {format} (use json or xml)"));
    }
    serde_json::to_string_pretty(&json!({
        "skills": report.skills.iter().map(|s| json!({
            "name": s.name,
            "description": s.description,
            "source": s.source,
            "path": s.source_path,
            "user_invocable": s.user_invocable,
            "disable_model_invocation": s.disable_model_invocation,
        })).collect::<Vec<_>>(),
        "skips": report.skips,
    }))
    .map_err(|e| e.to_string())
}

fn load_text(args: LoadArgs) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let report = discover(
        &cwd,
        &opts_from(args.paths, args.vendor, args.user_dir, args.ascii_names)?,
    )
    .map_err(|e| e.to_string())?;
    match find_skill_by_name(&report.skills, &args.name) {
        Some(skill) => Ok(format_load_message(
            skill,
            args.args.as_deref().unwrap_or(""),
            FormatOptions::default(),
        )),
        None => Err(unknown_or_skipped_skill_message(&args.name, &report.skips)),
    }
}

fn why_json(args: WhyArgs) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let report = discover(
        &cwd,
        &opts_from(args.paths, args.vendor, args.user_dir, args.ascii_names)?,
    )
    .map_err(|e| e.to_string())?;
    let budgets = progressive_budgets(args.context_tokens.unwrap_or(8_000));
    let why = why(
        &report,
        args.name.as_deref(),
        args.context.as_deref(),
        Some(budgets),
    );
    if let Some(msg) = why.unknown_skill_message() {
        return Err(msg);
    }
    serde_json::to_string_pretty(&why).map_err(|e| e.to_string())
}

fn discover_properties() -> Value {
    json!({
        "paths": {"type": "array", "items": {"type": "string"}, "description": "Extra SKILL.md package or collection roots. Example: [\"./my-skill\"]."},
        "vendor": {"type": "array", "items": {"type": "string"}, "description": "Opt-in vendor trees: bline, claude, cursor, grok. Example: [\"claude\"] or [\".claude\"]."},
        "user_dir": {"type": "string", "description": "User skills root (child dirs are packages). Example: \"~/myskills\"."},
        "ascii_names": {"type": "boolean", "description": "Reject names outside a-z0-9-. Default still allows Unicode / NFKC."}
    })
}

fn tools() -> Value {
    let mut list_props = discover_properties();
    list_props["format"] = json!({
        "type": "string",
        "description": "json (default) or xml (skills-ref <available_skills>)."
    });
    let mut load_props = discover_properties();
    load_props["name"] = json!({"type": "string", "description": "Frontmatter skill name."});
    load_props["args"] =
        json!({"type": "string", "description": "Optional arguments passed into the envelope."});
    let mut why_props = discover_properties();
    why_props["name"] = json!({"type": "string", "description": "Optional skill name filter."});
    why_props["context"] = json!({"type": "string", "description": "Activation context text."});
    why_props["context_tokens"] =
        json!({"type": "integer", "description": "Token budget for activation (default 8000)."});
    json!([
        {
            "name": "skills_list",
            "description": "List discovered skills.",
            "inputSchema": {"type": "object", "properties": list_props}
        },
        {
            "name": "skills_load",
            "description": "Load one skill body and package envelope. Does not dump scripts/ or references/ file bodies.",
            "inputSchema": {
                "type": "object",
                "required": ["name"],
                "properties": load_props
            }
        },
        {
            "name": "skills_why",
            "description": "Explain loaded, skipped, and activation decisions.",
            "inputSchema": {"type": "object", "properties": why_props}
        }
    ])
}

/// Decode tool arguments. Null/omitted is the empty object; a type
/// mismatch against `inputSchema` is an error (not a silent default).
/// Present `null` on a typed property is a type error, same as a
/// wrong JSON type. Omitted properties still use the field default.
fn tool_args<T>(value: Value) -> Result<T, String>
where
    T: Default + serde::de::DeserializeOwned,
{
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn handle(req: RpcRequest) -> Option<Value> {
    let method = req.method.unwrap_or_default();
    let id = req.id?;
    if req.jsonrpc.as_deref() != Some("2.0") && req.jsonrpc.is_some() {
        return Some(err(id, -32600, "invalid jsonrpc"));
    }
    match method.as_str() {
        "initialize" => Some(ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "craftbag", "version": env!("CARGO_PKG_VERSION")}
            }),
        )),
        "ping" => Some(ok(id, json!({}))),
        "tools/list" => Some(ok(id, json!({"tools": tools()}))),
        "tools/call" => {
            let params: CallParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Some(err(id, -32602, &e.to_string())),
            };
            let (text, is_err) = match params.name.as_str() {
                "skills_list" => match tool_args(params.arguments) {
                    Ok(args) => match list_json(args) {
                        Ok(s) => (s, false),
                        Err(e) => (e, true),
                    },
                    Err(e) => (e, true),
                },
                "skills_load" => match serde_json::from_value::<LoadArgs>(params.arguments) {
                    Ok(args) => match load_text(args) {
                        Ok(s) => (s, false),
                        Err(e) => (e, true),
                    },
                    Err(e) => (e.to_string(), true),
                },
                "skills_why" => match tool_args(params.arguments) {
                    Ok(args) => match why_json(args) {
                        Ok(s) => (s, false),
                        Err(e) => (e, true),
                    },
                    Err(e) => (e, true),
                },
                other => return Some(err(id, -32601, &format!("unknown tool: {other}"))),
            };
            Some(ok(
                id,
                json!({
                    "content": [{"type": "text", "text": text}],
                    "isError": is_err
                }),
            ))
        }
        other => Some(err(id, -32601, &format!("method not found: {other}"))),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(stdout, "{}", err(Value::Null, -32700, &e.to_string()));
                let _ = stdout.flush();
                continue;
            }
        };
        if let Some(resp) = handle(req) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiscoverArgs, RpcRequest, handle, list_json};
    use serde_json::json;
    use std::path::PathBuf;

    fn corpus_pkg() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/agentskills/minimal-valid")
            .to_string_lossy()
            .into_owned()
    }

    fn corpus_claude_user() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/incumbent/claude-user")
    }

    fn call(id: i64, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(id)),
            method: Some("tools/call".into()),
            params: json!({"name": name, "arguments": arguments}),
        })
        .expect("resp")
    }

    fn call_text(resp: &serde_json::Value) -> &str {
        resp["result"]["content"][0]["text"].as_str().expect("text")
    }

    fn empty_home<T>(f: impl FnOnce() -> T) -> T {
        let home = tempfile::tempdir().expect("home");
        craftbag::with_home_override(Some(home.path().to_path_buf()), f)
    }

    #[test]
    fn initialize_advertises_tools() {
        let req = RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(1)),
            method: Some("initialize".into()),
            params: json!({}),
        };
        let resp = handle(req).expect("resp");
        assert_eq!(resp["result"]["capabilities"]["tools"], json!({}));
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(2)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let got: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(got, ["skills_list", "skills_load", "skills_why"]);
        assert!(
            tools[1]["inputSchema"]["required"]
                .as_array()
                .expect("req")
                .iter()
                .any(|v| v == "name")
        );
    }

    #[test]
    fn notification_has_no_response() {
        let resp = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: None,
            method: Some("notifications/initialized".into()),
            params: json!({}),
        });
        assert!(resp.is_none());
    }

    #[test]
    fn list_json_shape() {
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![corpus_pkg()],
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert!(v.get("skills").is_some(), "{out}");
        assert!(v.get("skips").is_some(), "{out}");
        assert!(out.contains("minimal-valid"), "{out}");
        assert_eq!(
            v["skills"][0]["user_invocable"], true,
            "omitted user_invocable defaults true: {out}"
        );
        assert_eq!(
            v["skills"][0]["disable_model_invocation"], false,
            "omitted disable_model_invocation defaults false: {out}"
        );
    }

    #[test]
    fn list_json_vendor_claude_loads_user_home_layout() {
        let home = corpus_claude_user();
        let off = craftbag::with_home_override(Some(home.clone()), || {
            list_json(DiscoverArgs::default()).expect("list")
        });
        assert!(
            !off.contains("home-note"),
            "claude vendor is opt-in at HOME too: {off}"
        );
        let on = craftbag::with_home_override(Some(home), || {
            list_json(DiscoverArgs {
                vendor: vec!["claude".to_owned()],
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(
            on.contains("home-note"),
            "MCP list must find HOME/.claude/skills when vendor claude is on: {on}"
        );
    }

    #[test]
    fn list_json_includes_invocation_flags() {
        let extra = tempfile::tempdir().expect("extra");
        let hidden = extra.path().join("hidden-slash");
        std::fs::create_dir_all(&hidden).expect("mkdir");
        std::fs::write(
            hidden.join("SKILL.md"),
            "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\n---\nbody\n",
        )
        .expect("write");
        let slash = extra.path().join("slash-only");
        std::fs::create_dir_all(&slash).expect("mkdir");
        std::fs::write(
            slash.join("SKILL.md"),
            "---\nname: slash-only\ndescription: user only\ndisable-model-invocation: true\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![path],
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        let skills = v["skills"].as_array().expect("skills");
        let hidden_row = skills
            .iter()
            .find(|s| s["name"] == "hidden-slash")
            .expect("hidden-slash");
        assert_eq!(
            hidden_row["user_invocable"], false,
            "MCP list must carry user_invocable for slash palettes: {out}"
        );
        assert_eq!(hidden_row["disable_model_invocation"], false, "{out}");
        let slash_row = skills
            .iter()
            .find(|s| s["name"] == "slash-only")
            .expect("slash-only");
        assert_eq!(slash_row["user_invocable"], true, "{out}");
        assert_eq!(slash_row["disable_model_invocation"], true, "{out}");
    }

    #[test]
    fn why_json_includes_invocation_flags() {
        let extra = tempfile::tempdir().expect("extra");
        let hidden = extra.path().join("hidden-slash");
        std::fs::create_dir_all(&hidden).expect("mkdir");
        std::fs::write(
            hidden.join("SKILL.md"),
            "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\n---\nbody\n",
        )
        .expect("write");
        let slash = extra.path().join("slash-only");
        std::fs::create_dir_all(&slash).expect("mkdir");
        std::fs::write(
            slash.join("SKILL.md"),
            "---\nname: slash-only\ndescription: user only\ndisable-model-invocation: true\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let why_text = empty_home(|| {
            let why = call(40, "skills_why", json!({"paths": [path]}));
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            call_text(&why).to_owned()
        });
        let v: serde_json::Value = serde_json::from_str(&why_text).expect("why json");
        let loaded = v["loaded"].as_array().expect("loaded");
        let hidden_row = loaded
            .iter()
            .find(|s| s["name"] == "hidden-slash")
            .expect("hidden-slash");
        assert_eq!(
            hidden_row["user_invocable"], false,
            "MCP why must carry user_invocable like list JSON/XML: {why_text}"
        );
        assert_eq!(hidden_row["disable_model_invocation"], false, "{why_text}");
        assert!(
            hidden_row.get("userInvocable").is_none(),
            "why JSON flags must match list snake_case, not Skill camelCase: {why_text}"
        );
        let slash_row = loaded
            .iter()
            .find(|s| s["name"] == "slash-only")
            .expect("slash-only");
        assert_eq!(slash_row["user_invocable"], true, "{why_text}");
        assert_eq!(slash_row["disable_model_invocation"], true, "{why_text}");
    }

    #[test]
    fn list_unknown_format_names_json_and_xml() {
        empty_home(|| {
            let err = list_json(DiscoverArgs {
                format: Some("yaml".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect_err("format");
            assert!(err.contains("unknown format: yaml"), "{err}");
            assert!(
                err.contains("json") && err.contains("xml"),
                "must name valid formats: {err}"
            );
        });
    }

    #[test]
    fn skills_list_unknown_vendor_is_error() {
        empty_home(|| {
            let resp = call(49, "skills_list", json!({"vendor": ["nope"]}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "unknown vendor must not look like an empty catalog: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(text.contains("unknown vendor: nope"), "{text}");
            assert!(text.contains("claude"), "{text}");
        });
    }

    #[test]
    fn list_xml_shape() {
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![corpus_pkg()],
                format: Some("xml".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(out.contains("<available_skills>"), "{out}");
        assert!(out.contains("<name>minimal-valid</name>"), "{out}");
        assert!(
            out.contains("<user_invocable>true</user_invocable>"),
            "omitted user_invocable defaults true: {out}"
        );
        assert!(
            out.contains("<disable_model_invocation>false</disable_model_invocation>"),
            "omitted disable_model_invocation defaults false: {out}"
        );
    }

    #[test]
    fn list_xml_includes_invocation_flags() {
        let extra = tempfile::tempdir().expect("extra");
        let hidden = extra.path().join("hidden-slash");
        std::fs::create_dir_all(&hidden).expect("mkdir");
        std::fs::write(
            hidden.join("SKILL.md"),
            "---\nname: hidden-slash\ndescription: model only\nuser_invocable: false\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![path],
                format: Some("xml".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(out.contains("<name>hidden-slash</name>"), "{out}");
        assert!(
            out.contains("<user_invocable>false</user_invocable>"),
            "MCP list XML must carry user_invocable for slash palettes: {out}"
        );
        assert!(
            out.contains("<disable_model_invocation>false</disable_model_invocation>"),
            "MCP list XML must carry disable_model_invocation: {out}"
        );
    }

    #[test]
    fn skills_list_and_load_and_why() {
        empty_home(|| {
            let pkg = corpus_pkg();
            let list = call(3, "skills_list", json!({"paths": [pkg.clone()]}));
            assert_eq!(list["result"]["isError"], false);
            assert!(call_text(&list).contains("minimal-valid"));

            let load = call(
                4,
                "skills_load",
                json!({"name": "minimal-valid", "paths": [pkg.clone()]}),
            );
            assert_eq!(load["result"]["isError"], false);
            let load_text = call_text(&load);
            assert!(
                load_text.contains("[Activated skill: minimal-valid]"),
                "{load_text}"
            );

            let why = call(5, "skills_why", json!({"paths": [pkg]}));
            assert_eq!(why["result"]["isError"], false);
            let why_text = call_text(&why);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert!(why_v.get("loaded").is_some(), "{why_text}");
            assert!(why_v.get("skips").is_some(), "{why_text}");
            assert!(why_v.get("activation").is_some(), "{why_text}");
        });
    }

    #[test]
    fn skills_load_unknown_is_error() {
        empty_home(|| {
            let resp = call(6, "skills_load", json!({"name": "no-such-skill"}));
            assert_eq!(resp["result"]["isError"], true);
            let text = call_text(&resp);
            assert!(text.contains("unknown skill"), "{text}");
            assert!(!text.contains("skipped skill"), "{text}");
        });
    }

    #[test]
    fn skills_load_parse_error_skip_is_not_unknown() {
        empty_home(|| {
            let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/corpus/agentskills/invalid-name")
                .to_string_lossy()
                .into_owned();
            let resp = call(
                8,
                "skills_load",
                json!({"name": "Bad_Name", "paths": [parent]}),
            );
            assert_eq!(resp["result"]["isError"], true);
            let text = call_text(&resp);
            assert!(
                text.contains("skipped skill: Bad_Name"),
                "load must name the skipped skill: {text}"
            );
            assert!(text.contains("parse_error"), "{text}");
            assert!(
                !text.contains("unknown skill"),
                "skipped parse error must not look missing: {text}"
            );
        });
    }

    #[test]
    fn skills_load_parse_skip_without_frontmatter_name_is_not_unknown() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let pkg = tmp.path().join("demo");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\ndescription: no name\n---\nbody\n",
            )
            .expect("write");
            let parent = tmp.path().to_string_lossy().into_owned();
            let resp = call(9, "skills_load", json!({"name": "demo", "paths": [parent]}));
            assert_eq!(resp["result"]["isError"], true);
            let text = call_text(&resp);
            assert!(
                text.contains("skipped skill: demo"),
                "load must use the package dir when peek name is missing: {text}"
            );
            assert!(text.contains("parse_error"), "{text}");
            assert!(
                !text.contains("unknown skill"),
                "nameless parse skip must not look missing: {text}"
            );
        });
    }

    #[test]
    fn skills_load_named_root_file_skip_is_not_unknown() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let skills = tmp.path().join("skills");
            std::fs::create_dir_all(&skills).expect("mkdir");
            std::fs::write(
                skills.join("SKILL.md"),
                "---\nname: loose\ndescription: loose\n---\nbody\n",
            )
            .expect("write");
            let resp = call(
                17,
                "skills_load",
                json!({"name": "loose", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], true);
            let text = call_text(&resp);
            assert!(
                text.contains("skipped skill: loose"),
                "load must name the root-file skip: {text}"
            );
            assert!(text.contains("root_file"), "{text}");
            assert!(
                !text.contains("unknown skill"),
                "named root-file skip must not look missing: {text}"
            );
        });
    }

    #[test]
    fn skills_why_named_root_file_skip_is_not_unknown() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let skills = tmp.path().join("skills");
            std::fs::create_dir_all(&skills).expect("mkdir");
            std::fs::write(
                skills.join("SKILL.md"),
                "---\nname: loose\ndescription: loose\n---\nbody\n",
            )
            .expect("write");
            let why = call(
                18,
                "skills_why",
                json!({"name": "loose", "paths": [tmp.path()]}),
            );
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            let why_text = call_text(&why);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert_eq!(why_v["skips"][0]["name"], "loose", "{why_text}");
            assert_eq!(why_v["skips"][0]["kind"], "root_file", "{why_text}");
        });
    }

    #[test]
    fn skills_load_extra_path_root_file_does_not_hide_package() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            std::fs::write(
                tmp.path().join("SKILL.md"),
                "---\nname: demo\ndescription: loose\n---\nloose\n",
            )
            .expect("write");
            let pkg = tmp.path().join("demo");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: demo\ndescription: package\n---\npackage body\n",
            )
            .expect("write");
            let resp = call(
                19,
                "skills_load",
                json!({"name": "demo", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: demo]"),
                "package must load: {text}"
            );
            assert!(
                text.contains("package body"),
                "must load the named package, not the loose file: {text}"
            );
        });
    }

    #[test]
    fn skills_why_extra_path_root_file_and_package_agree_with_load() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            std::fs::write(
                tmp.path().join("SKILL.md"),
                "---\nname: demo\ndescription: loose\n---\nloose\n",
            )
            .expect("write");
            let pkg = tmp.path().join("demo");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: demo\ndescription: package\n---\npackage body\n",
            )
            .expect("write");
            let why = call(
                20,
                "skills_why",
                json!({"name": "demo", "paths": [tmp.path()]}),
            );
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            let why_text = call_text(&why);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert_eq!(why_v["loaded"][0]["name"], "demo", "{why_text}");
            assert_eq!(why_v["skips"][0]["name"], "demo", "{why_text}");
            assert_eq!(why_v["skips"][0]["kind"], "root_file", "{why_text}");
        });
    }

    #[test]
    fn skills_why_unknown_is_error() {
        empty_home(|| {
            let resp = call(10, "skills_why", json!({"name": "no-such-skill"}));
            assert_eq!(resp["result"]["isError"], true);
            let text = call_text(&resp);
            assert!(text.contains("unknown skill: no-such-skill"), "{text}");
        });
    }

    #[test]
    fn skills_why_whitespace_name_is_error_not_empty_json() {
        empty_home(|| {
            let resp = call(12, "skills_why", json!({"name": "   "}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "whitespace name must not return empty why JSON: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("unknown skill"),
                "whitespace name is unknown, not an omitted filter: {text}"
            );
        });
    }

    #[test]
    fn skills_why_parse_skip_without_frontmatter_name_is_not_unknown() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let pkg = tmp.path().join("demo");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\ndescription: no name\n---\nbody\n",
            )
            .expect("write");
            let parent = tmp.path().to_string_lossy().into_owned();
            let why = call(11, "skills_why", json!({"name": "demo", "paths": [parent]}));
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            let why_text = call_text(&why);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert_eq!(why_v["skips"][0]["kind"], "parse_error", "{why_text}");
            assert_eq!(why_v["skips"][0]["code"], "parse_error", "{why_text}");
            assert!(
                why_text.contains("demo"),
                "why JSON must keep the package path: {why_text}"
            );
        });
    }

    #[test]
    fn skills_why_parse_error_keeps_name() {
        empty_home(|| {
            let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/corpus/agentskills/invalid-name")
                .to_string_lossy()
                .into_owned();
            let why = call(
                7,
                "skills_why",
                json!({"name": "Bad_Name", "paths": [parent]}),
            );
            assert_eq!(why["result"]["isError"], false);
            let why_text = call_text(&why);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert_eq!(why_v["skips"][0]["name"], "Bad_Name", "{why_text}");
            assert_eq!(why_v["skips"][0]["kind"], "parse_error", "{why_text}");
        });
    }

    #[test]
    fn skills_list_invalid_paths_type_is_error_not_default_catalog() {
        empty_home(|| {
            let resp = call(13, "skills_list", json!({"paths": "/not/an/array"}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says paths is string[]; must not list cwd: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return a default catalog JSON: {text}"
            );
        });
    }

    #[test]
    fn skills_why_invalid_context_tokens_type_is_error_not_default_report() {
        empty_home(|| {
            let resp = call(
                14,
                "skills_why",
                json!({"name": "demo", "context_tokens": "8000"}),
            );
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says context_tokens is integer; must not run default why: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"loaded\""),
                "must not return default why JSON: {text}"
            );
        });
    }

    #[test]
    fn skills_why_invalid_name_type_is_error() {
        empty_home(|| {
            let resp = call(15, "skills_why", json!({"name": 123}));
            assert_eq!(resp["result"]["isError"], true, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                !text.contains("\"activation\""),
                "must not return why JSON for a non-string name: {text}"
            );
        });
    }

    #[test]
    fn skills_list_null_arguments_still_lists() {
        empty_home(|| {
            let resp = call(16, "skills_list", serde_json::Value::Null);
            assert_eq!(
                resp["result"]["isError"],
                false,
                "omitted arguments are the empty object: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            let v: serde_json::Value = serde_json::from_str(text).expect("list json");
            assert!(v.get("skills").is_some(), "{text}");
            assert!(v.get("skips").is_some(), "{text}");
        });
    }

    #[test]
    fn skills_why_null_name_is_error_not_unfiltered_report() {
        empty_home(|| {
            let resp = call(21, "skills_why", json!({"name": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says name is string; null must not mean omit-filter: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"loaded\""),
                "must not return default why JSON for null name: {text}"
            );
        });
    }

    #[test]
    fn skills_why_null_context_tokens_is_error_not_default_budget() {
        empty_home(|| {
            let resp = call(22, "skills_why", json!({"context_tokens": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says context_tokens is integer; null must not default: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"loaded\""),
                "must not return default why JSON for null context_tokens: {text}"
            );
        });
    }

    #[test]
    fn skills_why_null_context_is_error_not_no_activation() {
        empty_home(|| {
            let resp = call(24, "skills_why", json!({"context": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says context is string; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"activation\""),
                "must not return why JSON for null context: {text}"
            );
        });
    }

    #[test]
    fn skills_load_null_args_is_error_not_empty_args() {
        empty_home(|| {
            let pkg = corpus_pkg();
            let resp = call(
                25,
                "skills_load",
                json!({"name": "minimal-valid", "paths": [pkg], "args": null}),
            );
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says args is string; null must not mean empty: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("[Activated skill:"),
                "must not load with default empty args: {text}"
            );
        });
    }

    #[test]
    fn skills_list_user_dir_tilde_expands() {
        let home = tempfile::tempdir().expect("home");
        let pkg = home.path().join("myskills").join("mine");
        std::fs::create_dir_all(&pkg).expect("mkdir");
        std::fs::write(
            pkg.join("SKILL.md"),
            "---\nname: mine\ndescription: from-home\n---\nfrom-home\n",
        )
        .expect("write");
        craftbag::with_home_override(Some(home.path().to_path_buf()), || {
            let resp = call(27, "skills_list", json!({"user_dir": "~/myskills"}));
            assert_eq!(
                resp["result"]["isError"],
                false,
                "user_dir ~/myskills must expand like paths: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("mine"),
                "MCP user_dir tilde must load the home skill: {text}"
            );
        });
    }

    #[test]
    fn skills_list_null_user_dir_is_error() {
        empty_home(|| {
            let resp = call(26, "skills_list", json!({"user_dir": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says user_dir is string; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog for null user_dir: {text}"
            );
        });
    }

    #[test]
    fn skills_list_null_format_is_error_not_json_default() {
        empty_home(|| {
            let resp = call(23, "skills_list", json!({"format": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says format is string; null must not default to json: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog JSON for null format: {text}"
            );
        });
    }

    #[test]
    fn skills_list_empty_user_dir_is_error_not_cwd() {
        empty_home(|| {
            let resp = call(40, "skills_list", json!({"user_dir": ""}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "empty user_dir must not scan cwd: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("user_dir"),
                "error must name user_dir: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return a catalog for empty user_dir: {text}"
            );
        });
    }

    #[test]
    fn skills_list_whitespace_user_dir_is_error_not_cwd() {
        empty_home(|| {
            let resp = call(41, "skills_list", json!({"user_dir": "   "}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "whitespace user_dir must not scan cwd: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("user_dir"),
                "error must name user_dir: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return a catalog for whitespace user_dir: {text}"
            );
        });
    }

    #[test]
    fn skills_load_extra_path_root_skill_md_does_not_hide_skills_subdir() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            std::fs::write(
                tmp.path().join("SKILL.md"),
                "---\nname: loose\ndescription: leftover\n---\nloose\n",
            )
            .expect("write");
            let pkg = tmp.path().join("skills").join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: from-skills\n---\nfrom-skills\n",
            )
            .expect("write");
            let resp = call(
                28,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find skills/public behind a leftover extra-path SKILL.md: {text}"
            );
            assert!(
                text.contains("from-skills"),
                "must load the skills/ package, not the leftover root file: {text}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn skills_load_escaped_extra_path_root_does_not_hide_sibling() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let outside = tempfile::tempdir().expect("out");
            std::fs::write(
                outside.path().join("secret.md"),
                "---\nname: stolen\ndescription: leaked\n---\nSECRET_BODY\n",
            )
            .expect("write");
            std::os::unix::fs::symlink(
                outside.path().join("secret.md"),
                tmp.path().join("SKILL.md"),
            )
            .expect("symlink");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let resp = call(
                29,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public behind an escaped extra-path SKILL.md: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the escaped root file: {text}"
            );
            assert!(
                !text.contains("SECRET_BODY"),
                "must not load the escaped SKILL.md body: {text}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn skills_load_escaped_extra_path_skills_does_not_hide_sibling() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let outside = tempfile::tempdir().expect("out");
            std::fs::create_dir_all(outside.path().join("stolen")).expect("mkdir");
            std::fs::write(
                outside.path().join("stolen").join("SKILL.md"),
                "---\nname: stolen\ndescription: leaked\n---\nSECRET_BODY\n",
            )
            .expect("write");
            std::os::unix::fs::symlink(outside.path(), tmp.path().join("skills")).expect("symlink");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let resp = call(
                30,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public when extra/skills/ escapes: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the escaped skills/ tree: {text}"
            );
            assert!(
                !text.contains("SECRET_BODY"),
                "must not load the escaped skills/ body: {text}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn skills_load_unreadable_extra_path_skills_does_not_hide_sibling() {
        use std::os::unix::fs::PermissionsExt;
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let skills = tmp.path().join("skills");
            std::fs::create_dir_all(skills.join("hidden")).expect("mkdir");
            std::fs::write(
                skills.join("hidden").join("SKILL.md"),
                "---\nname: hidden\ndescription: locked\n---\nlocked\n",
            )
            .expect("write");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let original = std::fs::metadata(&skills).expect("meta").permissions();
            struct Restore<'a>(&'a std::path::Path, std::fs::Permissions);
            impl Drop for Restore<'_> {
                fn drop(&mut self) {
                    let _ = std::fs::set_permissions(self.0, self.1.clone());
                }
            }
            let _restore = Restore(&skills, original.clone());
            let mut locked = original.clone();
            locked.set_mode(0o000);
            std::fs::set_permissions(&skills, locked).expect("chmod");
            if std::fs::read_dir(&skills).is_ok() {
                return;
            }
            let resp = call(
                31,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public when extra/skills/ is unreadable: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the locked skills/ tree: {text}"
            );
        });
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
    fn skills_load_fifo_leftover_extra_path_skill_md_does_not_hide_skills_subdir() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            mkfifo(&tmp.path().join("SKILL.md"));
            let pkg = tmp.path().join("skills").join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: from-skills\n---\nfrom-skills\n",
            )
            .expect("write");
            let resp = call(
                32,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find skills/public behind a leftover extra-path FIFO SKILL.md: {text}"
            );
            assert!(
                text.contains("from-skills"),
                "must load the skills/ package, not the leftover FIFO: {text}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn skills_load_fifo_leftover_and_skills_file_does_not_hide_sibling() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            mkfifo(&tmp.path().join("SKILL.md"));
            std::fs::write(tmp.path().join("skills"), "not-a-dir").expect("skills file");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let resp = call(
                33,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public when leftover extra/SKILL.md is a FIFO and extra/skills is a file: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the leftover FIFO: {text}"
            );
        });
    }

    #[test]
    fn skills_load_nameless_leftover_and_skills_file_does_not_hide_sibling() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            std::fs::write(
                tmp.path().join("SKILL.md"),
                "---\ndescription: leftover without name\n---\nloose\n",
            )
            .expect("write");
            std::fs::write(tmp.path().join("skills"), "not-a-dir").expect("skills file");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let resp = call(
                34,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public when leftover extra/SKILL.md has no name and extra/skills is a file: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the nameless leftover: {text}"
            );
        });
    }

    #[test]
    fn skills_load_blank_peek_leftover_and_skills_file_does_not_hide_sibling() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            std::fs::write(
                tmp.path().join("SKILL.md"),
                "---\nname: \"   \"\ndescription: leftover blank name\n---\nloose\n",
            )
            .expect("write");
            std::fs::write(tmp.path().join("skills"), "not-a-dir").expect("skills file");
            let pkg = tmp.path().join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: sibling\n---\nfrom-sibling\n",
            )
            .expect("write");
            let resp = call(
                35,
                "skills_load",
                json!({"name": "public", "paths": [tmp.path()]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find sibling public when leftover extra/SKILL.md peeks a blank name and extra/skills is a file: {text}"
            );
            assert!(
                text.contains("from-sibling"),
                "must load the sibling package, not the blank-peek leftover: {text}"
            );
        });
    }

    #[test]
    fn skills_load_invalid_matching_peek_leftover_and_skills_dir_does_not_hide_collection() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let extra = tmp.path().join("demo");
            std::fs::create_dir_all(&extra).expect("mkdir extra");
            std::fs::write(
                extra.join("SKILL.md"),
                "---\nname: DEMO\ndescription: leftover invalid name\n---\nloose\n",
            )
            .expect("write");
            let pkg = extra.join("skills").join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
            )
            .expect("write");
            let resp = call(
                36,
                "skills_load",
                json!({"name": "public", "paths": [extra]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find skills/public when leftover extra/SKILL.md peeks DEMO matching extra dir: {text}"
            );
            assert!(
                text.contains("from-collection"),
                "must load the skills/ package, not the invalid-matching leftover: {text}"
            );
        });
    }

    #[test]
    fn skills_load_unparseable_matching_peek_leftover_and_skills_dir_does_not_hide_collection() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let extra = tmp.path().join("demo");
            std::fs::create_dir_all(&extra).expect("mkdir extra");
            std::fs::write(extra.join("SKILL.md"), "---\nname: demo\n---\nloose\n").expect("write");
            let pkg = extra.join("skills").join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
            )
            .expect("write");
            let resp = call(
                38,
                "skills_load",
                json!({"name": "public", "paths": [extra]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find skills/public when leftover extra/SKILL.md peeks demo matching extra dir but cannot parse: {text}"
            );
            assert!(
                text.contains("from-collection"),
                "must load the skills/ package, not the unparseable leftover: {text}"
            );
        });
    }

    #[test]
    fn skills_load_path_component_peek_leftover_and_skills_dir_does_not_hide_collection() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let extra = tmp.path().join("wanted");
            std::fs::create_dir_all(&extra).expect("mkdir extra");
            std::fs::write(
                extra.join("SKILL.md"),
                "---\nname: .\ndescription: leftover path-like name\n---\nloose\n",
            )
            .expect("write");
            let pkg = extra.join("skills").join("public");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: public\ndescription: collection\n---\nfrom-collection\n",
            )
            .expect("write");
            let resp = call(
                37,
                "skills_load",
                json!({"name": "public", "paths": [extra]}),
            );
            assert_eq!(resp["result"]["isError"], false, "{}", call_text(&resp));
            let text = call_text(&resp);
            assert!(
                text.contains("[Activated skill: public]"),
                "MCP load must find skills/public when leftover extra/SKILL.md peeks `.`: {text}"
            );
            assert!(
                text.contains("from-collection"),
                "must load the skills/ package, not the path-component leftover: {text}"
            );
        });
    }

    fn write_cafe_extra() -> tempfile::TempDir {
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("café");
        std::fs::create_dir_all(&pkg).expect("mkdir");
        std::fs::write(
            pkg.join("SKILL.md"),
            "---\nname: café\ndescription: coffee\n---\nbody\n",
        )
        .expect("write");
        extra
    }

    #[test]
    fn skills_list_load_why_ascii_names_skips_unicode_like_cli() {
        empty_home(|| {
            let extra = write_cafe_extra();
            let path = extra.path();

            let listed = call(42, "skills_list", json!({"paths": [path]}));
            assert_eq!(listed["result"]["isError"], false, "{}", call_text(&listed));
            let listed_text = call_text(&listed);
            assert!(
                listed_text.contains("café"),
                "omitted ascii_names must still load café like CLI default: {listed_text}"
            );

            let load_ok = call(43, "skills_load", json!({"name": "café", "paths": [path]}));
            assert_eq!(
                load_ok["result"]["isError"],
                false,
                "{}",
                call_text(&load_ok)
            );
            assert!(
                call_text(&load_ok).contains("[Activated skill: café]"),
                "omitted ascii_names must load café: {}",
                call_text(&load_ok)
            );

            let listed_ascii = call(
                44,
                "skills_list",
                json!({"paths": [path], "ascii_names": true}),
            );
            assert_eq!(
                listed_ascii["result"]["isError"],
                false,
                "{}",
                call_text(&listed_ascii)
            );
            let listed_ascii_text = call_text(&listed_ascii);
            let listed_v: serde_json::Value =
                serde_json::from_str(listed_ascii_text).expect("list json");
            assert!(
                listed_v["skills"].as_array().is_some_and(|s| s.is_empty()),
                "ascii_names must not list café: {listed_ascii_text}"
            );
            assert_eq!(
                listed_v["skips"][0]["kind"], "parse_error",
                "ascii_names must skip café as parse_error: {listed_ascii_text}"
            );
            assert_eq!(listed_v["skips"][0]["name"], "café", "{listed_ascii_text}");

            let load_ascii = call(
                45,
                "skills_load",
                json!({"name": "café", "paths": [path], "ascii_names": true}),
            );
            assert_eq!(
                load_ascii["result"]["isError"],
                true,
                "ascii_names load must not return café: {}",
                call_text(&load_ascii)
            );
            let load_text = call_text(&load_ascii);
            assert!(
                load_text.contains("skipped skill: café"),
                "ascii_names load must name the skip: {load_text}"
            );
            assert!(
                load_text.contains("parse_error"),
                "ascii_names load must be parse_error like CLI --ascii-names: {load_text}"
            );
            assert!(
                !load_text.contains("unknown skill"),
                "ascii_names skip must not look missing: {load_text}"
            );

            let why_ascii = call(
                46,
                "skills_why",
                json!({"name": "café", "paths": [path], "ascii_names": true}),
            );
            assert_eq!(
                why_ascii["result"]["isError"],
                false,
                "ascii_names why must report the skip, not unknown: {}",
                call_text(&why_ascii)
            );
            let why_text = call_text(&why_ascii);
            let why_v: serde_json::Value = serde_json::from_str(why_text).expect("why json");
            assert!(why_v["loaded"].as_array().is_some_and(|s| s.is_empty()));
            assert_eq!(why_v["skips"][0]["kind"], "parse_error", "{why_text}");
            assert_eq!(why_v["skips"][0]["name"], "café", "{why_text}");
        });
    }

    #[test]
    fn skills_list_null_ascii_names_is_error() {
        empty_home(|| {
            let resp = call(47, "skills_list", json!({"ascii_names": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says ascii_names is boolean; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog for null ascii_names: {text}"
            );
        });
    }

    #[test]
    fn tools_list_advertises_ascii_names() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(48)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for tool in tools {
            let props = &tool["inputSchema"]["properties"];
            assert!(
                props.get("ascii_names").is_some(),
                "{} must advertise ascii_names like CLI --ascii-names: {props}",
                tool["name"]
            );
            assert_eq!(
                props["ascii_names"]["type"], "boolean",
                "{} ascii_names type: {props}",
                tool["name"]
            );
        }
    }
}
