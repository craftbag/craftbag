//! MCP stdio server: `skills_list`, `skills_load`, `skills_why`.
//!
//! Official `rmcp` 1.0+ uses let-chains and does not compile on MSRV 1.85.
//! This binary speaks MCP JSON-RPC over stdio and wraps the same library
//! the CLI uses.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use craftbag::{
    DiscoveryOptions, FormatOptions, ListFormat, SkillMiss, SkillSource, SkillSummary, discover,
    find_skill_by_name, format_available_skills_xml, format_catalog, format_load_message,
    parse_list_format, progressive_budgets, unknown_or_skipped_skill, watch_dirs, why,
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
    /// Walk cwd-to-git .agents / vendor trees and HOME/.agents / vendor trees. Omitted is true.
    #[serde(default, deserialize_with = "present_non_null")]
    implicit_roots: Option<bool>,
    /// Skill names never loaded (silent; no skip row). Same NFKC identity as load / why.
    #[serde(default)]
    disabled: Vec<String>,
    /// Path prefixes never loaded (silent; no skip row). Relative prefixes join cwd.
    #[serde(default)]
    ignore: Vec<String>,
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
    /// Walk cwd-to-git .agents / vendor trees and HOME/.agents / vendor trees. Omitted is true.
    #[serde(default, deserialize_with = "present_non_null")]
    implicit_roots: Option<bool>,
    /// Skill names never loaded (silent; no skip row). Same NFKC identity as load / why.
    #[serde(default)]
    disabled: Vec<String>,
    /// Path prefixes never loaded (silent; no skip row). Relative prefixes join cwd.
    #[serde(default)]
    ignore: Vec<String>,
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
    /// Walk cwd-to-git .agents / vendor trees and HOME/.agents / vendor trees. Omitted is true.
    #[serde(default, deserialize_with = "present_non_null")]
    implicit_roots: Option<bool>,
    /// Skill names never loaded (silent; no skip row). Same NFKC identity as load / why.
    #[serde(default)]
    disabled: Vec<String>,
    /// Path prefixes never loaded (silent; no skip row). Relative prefixes join cwd.
    #[serde(default)]
    ignore: Vec<String>,
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
    implicit_roots: Option<bool>,
    disabled: Vec<String>,
    ignore: Vec<String>,
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
        ignore,
        disabled,
        vendor_roots: SkillSource::parse_vendor_roots(vendor)?,
        user_skills_dir: user_dir.map(PathBuf::from),
        ascii_names,
        implicit_roots: implicit_roots.unwrap_or(true),
    })
}

fn list_json(args: DiscoverArgs) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let opts = opts_from(
        args.paths,
        args.vendor,
        args.user_dir,
        args.ascii_names,
        args.implicit_roots,
        args.disabled,
        args.ignore,
    )?;
    let format = args.format.as_deref().unwrap_or("json");
    let format = parse_list_format(format)?;
    if format == ListFormat::Watch {
        let mut out = String::new();
        for dir in watch_dirs(&cwd, &opts) {
            out.push_str(&dir.display().to_string());
            out.push('\n');
        }
        return Ok(out);
    }
    let report = discover(&cwd, &opts).map_err(|e| e.to_string())?;
    if format == ListFormat::Xml {
        return Ok(format_available_skills_xml(&report.skills));
    }
    if format == ListFormat::Catalog {
        return Ok(format_catalog(
            &report.skills,
            "",
            progressive_budgets(8_000),
            FormatOptions::default(),
        ));
    }
    serde_json::to_string_pretty(&json!({
        "skills": report.skills.iter().map(SkillSummary::from).collect::<Vec<_>>(),
        "skips": report.skips,
    }))
    .map_err(|e| e.to_string())
}

struct ToolError {
    message: String,
    miss: Option<SkillMiss>,
}

impl From<String> for ToolError {
    fn from(message: String) -> Self {
        Self {
            message,
            miss: None,
        }
    }
}

impl From<SkillMiss> for ToolError {
    fn from(miss: SkillMiss) -> Self {
        Self {
            message: miss.error.clone(),
            miss: Some(miss),
        }
    }
}

fn load_text(args: LoadArgs) -> Result<String, ToolError> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let report = discover(
        &cwd,
        &opts_from(
            args.paths,
            args.vendor,
            args.user_dir,
            args.ascii_names,
            args.implicit_roots,
            args.disabled,
            args.ignore,
        )?,
    )
    .map_err(|e| e.to_string())?;
    match find_skill_by_name(&report.skills, &args.name) {
        Some(skill) => Ok(format_load_message(
            skill,
            args.args.as_deref().unwrap_or(""),
            FormatOptions::default(),
        )),
        None => Err(unknown_or_skipped_skill(&args.name, &report.skips).into()),
    }
}

fn why_json(args: WhyArgs) -> Result<String, ToolError> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let report = discover(
        &cwd,
        &opts_from(
            args.paths,
            args.vendor,
            args.user_dir,
            args.ascii_names,
            args.implicit_roots,
            args.disabled,
            args.ignore,
        )?,
    )
    .map_err(|e| e.to_string())?;
    let budgets = progressive_budgets(args.context_tokens.unwrap_or(8_000));
    let why = why(
        &report,
        args.name.as_deref(),
        args.context.as_deref(),
        Some(budgets),
    );
    if let Some(miss) = why.unknown_skill_miss() {
        return Err(miss.into());
    }
    Ok(serde_json::to_string_pretty(&why).map_err(|e| e.to_string())?)
}

fn tool_fail(err: ToolError) -> (String, bool, Option<SkillMiss>) {
    (err.message, true, err.miss)
}

/// Copy `{ error_kind, error }` and optional `path` from [`SkillMiss`] so
/// MCP cannot drop a peel key that CLI `why --json` / `validate --json`
/// already serialize.
fn merge_skill_miss(result: &mut Value, miss: &SkillMiss) {
    let peel = serde_json::to_value(miss).expect("SkillMiss serde");
    let obj = peel.as_object().expect("SkillMiss is a JSON object");
    for (key, value) in obj {
        result[key] = value.clone();
    }
}

fn discover_properties() -> Value {
    let vendor_tokens = SkillSource::VENDOR_TOKENS;
    let vendor_listed = vendor_tokens.join(", ");
    json!({
        "paths": {"type": "array", "items": {"type": "string"}, "description": "Extra SKILL.md package or collection roots. Example: [\"./my-skill\"]."},
        "vendor": {
            "type": "array",
            "items": {"type": "string", "enum": vendor_tokens},
            "description": format!(
                "Opt-in vendor trees: {vendor_listed}. A leading dot is the on-disk tree (same as CLI --vendor). Example: [\"claude\"]."
            )
        },
        "user_dir": {"type": "string", "description": "User skills root (child dirs are packages). Example: \"~/myskills\"."},
        "ascii_names": {"type": "boolean", "description": "Reject names outside a-z0-9-. Default still allows Unicode / NFKC."},
        "implicit_roots": {"type": "boolean", "description": "Walk cwd-to-git .agents / vendor trees and HOME/.agents / vendor trees. Omitted is true. False is collection-only (extra paths and user_dir still load)."},
        "disabled": {"type": "array", "items": {"type": "string"}, "description": "Skill names never loaded (silent; no skip row). Same NFKC identity as load / why. Example: [\"secret\"]."},
        "ignore": {"type": "array", "items": {"type": "string"}, "description": "Path prefixes never loaded (silent; no skip row). Relative prefixes join cwd. Example: [\"./secret\"]."}
    })
}

fn tools() -> Value {
    let mut list_props = discover_properties();
    list_props["format"] = json!({
        "type": "string",
        "enum": ListFormat::CANONICAL_TOKENS,
        "description": format!(
            "json (default `{{ skills, skips }}`), xml (skills-ref <available_skills>), catalog (markdown name + description), or watch (notify-watch roots; does not load SKILL.md). {} are the same walk as watch.",
            ListFormat::ALIAS_TOKENS.join(" and ")
        )
    });
    let mut load_props = discover_properties();
    load_props["name"] = json!({"type": "string", "description": "Frontmatter skill name."});
    load_props["args"] = json!({
        "type": "string",
        "description": "Optional arguments copied into the envelope as User arguments. Matches SKILL.md argument-hint when set."
    });
    let mut why_props = discover_properties();
    why_props["name"] = json!({"type": "string", "description": "Optional skill name filter."});
    why_props["context"] = json!({"type": "string", "description": "Activation context text."});
    why_props["context_tokens"] =
        json!({"type": "integer", "description": "Token budget for activation (default 8000)."});
    json!([
        {
            "name": "skills_list",
            "description": "List discovered skills. format is json (default `{ skills, skips }`), xml (skills-ref <available_skills>), catalog, or watch.",
            "inputSchema": {"type": "object", "properties": list_props}
        },
        {
            "name": "skills_load",
            "description": "Load one skill body and package envelope (includes argument-hint, when-to-use, and allowed-tools when set). Does not dump scripts/ or references/ file bodies. A miss sets isError and peels SkillMiss.error_kind plus error, and path when a skip is known (same as why --json).",
            "inputSchema": {
                "type": "object",
                "required": ["name"],
                "properties": load_props
            }
        },
        {
            "name": "skills_why",
            "description": "Explain loaded, skipped, and activation decisions. A name miss sets isError and peels SkillMiss.error_kind plus error.",
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
            let (text, is_err, miss) = match params.name.as_str() {
                "skills_list" => match tool_args(params.arguments) {
                    Ok(args) => match list_json(args) {
                        Ok(s) => (s, false, None),
                        Err(e) => tool_fail(e.into()),
                    },
                    Err(e) => tool_fail(e.into()),
                },
                "skills_load" => match serde_json::from_value::<LoadArgs>(params.arguments) {
                    Ok(args) => match load_text(args) {
                        Ok(s) => (s, false, None),
                        Err(e) => tool_fail(e),
                    },
                    Err(e) => tool_fail(e.to_string().into()),
                },
                "skills_why" => match tool_args(params.arguments) {
                    Ok(args) => match why_json(args) {
                        Ok(s) => (s, false, None),
                        Err(e) => tool_fail(e),
                    },
                    Err(e) => tool_fail(e.into()),
                },
                other => {
                    return Some(err(
                        id,
                        -32601,
                        &format!("unknown tool: {}", craftbag::sanitize_error_token(other)),
                    ));
                }
            };
            let mut result = json!({
                "content": [{"type": "text", "text": text}],
                "isError": is_err
            });
            if let Some(miss) = miss {
                merge_skill_miss(&mut result, &miss);
            }
            Some(ok(id, result))
        }
        other => Some(err(
            id,
            -32601,
            &format!(
                "method not found: {}",
                craftbag::sanitize_error_token(other)
            ),
        )),
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
    use std::fs;
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
        assert_eq!(
            v["skills"][0]["source"], "extra",
            "list JSON source must match list XML extra, not serde extraPath: {out}"
        );
    }

    #[test]
    fn list_catalog_prints_markdown_names() {
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![corpus_pkg()],
                format: Some("catalog".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(out.contains("## Skills"), "{out}");
        assert!(out.contains("minimal-valid"), "{out}");
        assert!(
            out.contains("Use the host activate command"),
            "catalog must stay host-neutral: {out}"
        );
    }

    #[test]
    fn list_watch_prints_extra_collection_dirs() {
        let extra = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/incumbent/vercel-npx");
        let extra_s = extra.display().to_string();
        let skills = extra.join("skills");
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![extra_s],
                format: Some("watch".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == extra.as_path()),
            "must watch the extra-path collection root: {out}"
        );
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == skills.as_path()),
            "must watch extra/skills when discover walks it: {out}"
        );
        assert!(
            !out.contains("deploy-hint") && !out.contains("## Skills"),
            "watch format must not load SKILL.md: {out}"
        );
    }

    #[test]
    fn list_watch_dirs_format_matches_watch() {
        let extra = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/incumbent/vercel-npx");
        let extra_s = extra.display().to_string();
        let skills = extra.join("skills");
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![extra_s],
                format: Some("watch-dirs".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == extra.as_path()),
            "watch-dirs must watch the extra-path collection root: {out}"
        );
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == skills.as_path()),
            "watch-dirs must watch extra/skills when discover walks it: {out}"
        );
        assert!(
            !out.contains("deploy-hint") && !out.contains("## Skills"),
            "watch-dirs format must not load SKILL.md: {out}"
        );
    }

    #[test]
    fn list_watch_prints_extra_path_skill_md_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let pkg = tmp.path().join("wanted");
        std::fs::create_dir_all(&pkg).expect("mkdir");
        let skill = pkg.join("SKILL.md");
        std::fs::write(&skill, "---\nname: wanted\ndescription: d\n---\nbody\n").expect("write");
        let skill_s = skill.display().to_string();
        let out = empty_home(|| {
            list_json(DiscoverArgs {
                paths: vec![skill_s],
                format: Some("watch".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == skill.as_path()),
            "must watch an extra-path SKILL.md file: {out}"
        );
        assert!(
            !out.contains("## Skills"),
            "watch format must not load SKILL.md: {out}"
        );
    }

    #[test]
    fn list_watch_vendor_claude_lists_user_home() {
        let home = corpus_claude_user();
        let want = home.join(".claude").join("skills");
        let out = craftbag::with_home_override(Some(home), || {
            list_json(DiscoverArgs {
                vendor: vec!["claude".to_owned()],
                format: Some("watch".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("list")
        });
        assert!(
            out.lines()
                .any(|l| std::path::Path::new(l) == want.as_path()),
            "must watch HOME/.claude/skills when vendor claude is on: {out}"
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
        assert_eq!(
            hidden_row["description"], "model only",
            "why JSON must carry description like list JSON/XML: {why_text}"
        );
        assert_eq!(slash_row["description"], "user only", "{why_text}");
    }

    #[test]
    fn list_json_includes_when_to_use() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("ranked");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: ranked\ndescription: hinted\nwhen-to-use: after rebase\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-when");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-when\ndescription: bare\n---\nbody\n",
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
        let hinted_row = skills
            .iter()
            .find(|s| s["name"] == "ranked")
            .expect("ranked");
        assert_eq!(
            hinted_row["when_to_use"], "after rebase",
            "MCP list must carry when_to_use for catalogs: {out}"
        );
        assert!(
            hinted_row.get("whenToUse").is_none(),
            "list JSON when_to_use must stay snake_case: {out}"
        );
        let bare_row = skills
            .iter()
            .find(|s| s["name"] == "no-when")
            .expect("no-when");
        assert!(
            bare_row["when_to_use"].is_null(),
            "omitted when_to_use is null on MCP list JSON: {out}"
        );
    }

    #[test]
    fn list_json_includes_argument_hint() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("slash-hint");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: slash-hint\ndescription: hinted\nargument-hint: [name]\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-hint");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-hint\ndescription: bare\n---\nbody\n",
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
        let hinted_row = skills
            .iter()
            .find(|s| s["name"] == "slash-hint")
            .expect("slash-hint");
        assert_eq!(
            hinted_row["argument_hint"], "[name]",
            "MCP list must carry argument_hint for slash palettes: {out}"
        );
        assert!(
            hinted_row.get("argumentHint").is_none(),
            "list JSON argument_hint must stay snake_case: {out}"
        );
        let bare_row = skills
            .iter()
            .find(|s| s["name"] == "no-hint")
            .expect("no-hint");
        assert!(
            bare_row["argument_hint"].is_null(),
            "omitted argument_hint is null on MCP list JSON: {out}"
        );
    }

    #[test]
    fn list_json_includes_allowed_tools() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("tools-ok");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: tools-ok\ndescription: hinted\nallowed-tools: Read Bash(git:*)\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-tools");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-tools\ndescription: bare\n---\nbody\n",
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
        let hinted_row = skills
            .iter()
            .find(|s| s["name"] == "tools-ok")
            .expect("tools-ok");
        assert_eq!(
            hinted_row["allowed_tools"], "Read Bash(git:*)",
            "MCP list must carry allowed_tools: {out}"
        );
        assert!(
            hinted_row.get("allowedTools").is_none(),
            "list JSON allowed_tools must stay snake_case: {out}"
        );
        let bare_row = skills
            .iter()
            .find(|s| s["name"] == "no-tools")
            .expect("no-tools");
        assert!(
            bare_row["allowed_tools"].is_null(),
            "omitted allowed_tools is null on MCP list JSON: {out}"
        );
    }

    #[test]
    fn why_json_includes_when_to_use() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("ranked");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: ranked\ndescription: hinted\nwhen_to_use: after rebase\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let why_text = empty_home(|| {
            let why = call(42, "skills_why", json!({"paths": [path]}));
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            call_text(&why).to_owned()
        });
        let v: serde_json::Value = serde_json::from_str(&why_text).expect("why json");
        let loaded = v["loaded"].as_array().expect("loaded");
        let hinted_row = loaded
            .iter()
            .find(|s| s["name"] == "ranked")
            .expect("ranked");
        assert_eq!(
            hinted_row["when_to_use"], "after rebase",
            "MCP why must carry when_to_use like list JSON/XML: {why_text}"
        );
    }

    #[test]
    fn why_json_includes_argument_hint() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("slash-hint");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: slash-hint\ndescription: hinted\nargument_hint: [name]\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let why_text = empty_home(|| {
            let why = call(41, "skills_why", json!({"paths": [path]}));
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            call_text(&why).to_owned()
        });
        let v: serde_json::Value = serde_json::from_str(&why_text).expect("why json");
        let loaded = v["loaded"].as_array().expect("loaded");
        let hinted_row = loaded
            .iter()
            .find(|s| s["name"] == "slash-hint")
            .expect("slash-hint");
        assert_eq!(
            hinted_row["argument_hint"], "[name]",
            "MCP why must carry argument_hint like list JSON/XML: {why_text}"
        );
    }

    #[test]
    fn why_json_includes_allowed_tools() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("tools-ok");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: tools-ok\ndescription: hinted\nallowed_tools: Read Bash(git:*)\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        let why_text = empty_home(|| {
            let why = call(43, "skills_why", json!({"paths": [path]}));
            assert_eq!(why["result"]["isError"], false, "{}", call_text(&why));
            call_text(&why).to_owned()
        });
        let v: serde_json::Value = serde_json::from_str(&why_text).expect("why json");
        let loaded = v["loaded"].as_array().expect("loaded");
        let hinted_row = loaded
            .iter()
            .find(|s| s["name"] == "tools-ok")
            .expect("tools-ok");
        assert_eq!(
            hinted_row["allowed_tools"], "Read Bash(git:*)",
            "MCP why must carry allowed_tools like list JSON/XML: {why_text}"
        );
    }

    #[test]
    fn skills_load_includes_when_to_use() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("ranked");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: ranked\ndescription: hinted\nwhen-to-use: after rebase\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-when");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-when\ndescription: bare\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        empty_home(|| {
            let hinted_load = call(
                52,
                "skills_load",
                json!({"name": "ranked", "paths": [path.clone()]}),
            );
            assert_eq!(
                hinted_load["result"]["isError"],
                false,
                "{}",
                call_text(&hinted_load)
            );
            let hinted_text = call_text(&hinted_load);
            assert!(
                hinted_text.contains("When to use: after rebase"),
                "MCP load must carry when_to_use like list JSON/XML: {hinted_text}"
            );
            let bare_load = call(
                53,
                "skills_load",
                json!({"name": "no-when", "paths": [path]}),
            );
            assert_eq!(
                bare_load["result"]["isError"],
                false,
                "{}",
                call_text(&bare_load)
            );
            let bare_text = call_text(&bare_load);
            assert!(
                !bare_text.contains("When to use:"),
                "omitted when_to_use must not add a load line: {bare_text}"
            );
        });
    }

    #[test]
    fn skills_load_includes_argument_hint() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("slash-hint");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: slash-hint\ndescription: hinted\nargument-hint: [name]\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-hint");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-hint\ndescription: bare\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        empty_home(|| {
            let hinted_load = call(
                50,
                "skills_load",
                json!({"name": "slash-hint", "args": "alice", "paths": [path.clone()]}),
            );
            assert_eq!(
                hinted_load["result"]["isError"],
                false,
                "{}",
                call_text(&hinted_load)
            );
            let hinted_text = call_text(&hinted_load);
            assert!(
                hinted_text.contains("Argument hint: [name]"),
                "MCP load must carry argument_hint like list JSON/XML: {hinted_text}"
            );
            assert!(
                hinted_text.contains("User arguments: alice"),
                "args still follow the hint: {hinted_text}"
            );
            let bare_load = call(
                51,
                "skills_load",
                json!({"name": "no-hint", "paths": [path]}),
            );
            assert_eq!(
                bare_load["result"]["isError"],
                false,
                "{}",
                call_text(&bare_load)
            );
            let bare_text = call_text(&bare_load);
            assert!(
                !bare_text.contains("Argument hint:"),
                "omitted argument_hint must not add a load line: {bare_text}"
            );
        });
    }

    #[test]
    fn skills_load_includes_allowed_tools() {
        let extra = tempfile::tempdir().expect("extra");
        let hinted = extra.path().join("tools-ok");
        std::fs::create_dir_all(&hinted).expect("mkdir");
        std::fs::write(
            hinted.join("SKILL.md"),
            "---\nname: tools-ok\ndescription: hinted\nallowed-tools: Read Bash(git:*)\n---\nbody\n",
        )
        .expect("write");
        let bare = extra.path().join("no-tools");
        std::fs::create_dir_all(&bare).expect("mkdir");
        std::fs::write(
            bare.join("SKILL.md"),
            "---\nname: no-tools\ndescription: bare\n---\nbody\n",
        )
        .expect("write");
        let path = extra.path().to_string_lossy().into_owned();
        empty_home(|| {
            let hinted_load = call(
                54,
                "skills_load",
                json!({"name": "tools-ok", "paths": [path.clone()]}),
            );
            assert_eq!(
                hinted_load["result"]["isError"],
                false,
                "{}",
                call_text(&hinted_load)
            );
            let hinted_text = call_text(&hinted_load);
            assert!(
                hinted_text.contains("Allowed tools: Read Bash(git:*)"),
                "MCP load must carry allowed_tools like list JSON/XML: {hinted_text}"
            );
            let bare_load = call(
                55,
                "skills_load",
                json!({"name": "no-tools", "paths": [path]}),
            );
            assert_eq!(
                bare_load["result"]["isError"],
                false,
                "{}",
                call_text(&bare_load)
            );
            let bare_text = call_text(&bare_load);
            assert!(
                !bare_text.contains("Allowed tools:"),
                "omitted allowed_tools must not add a load line: {bare_text}"
            );
        });
    }

    #[test]
    fn list_empty_format_names_valid_tokens() {
        empty_home(|| {
            let err = list_json(DiscoverArgs {
                format: Some("   ".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect_err("format");
            assert!(err.contains("unknown format: empty"), "{err}");
            assert!(
                err.contains("json")
                    && err.contains("xml")
                    && err.contains("catalog")
                    && err.contains("watch"),
                "must name valid formats: {err}"
            );
        });
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
                err.contains("json")
                    && err.contains("xml")
                    && err.contains("catalog")
                    && err.contains("watch"),
                "must name valid formats: {err}"
            );
        });
    }

    #[test]
    fn list_uppercase_format_suggests_lowercase() {
        empty_home(|| {
            let err = list_json(DiscoverArgs {
                format: Some("JSON".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect_err("format");
            assert!(
                err.contains("unknown format: JSON") && err.contains("did you mean json?"),
                "must point at the lowercase token: {err}"
            );
        });
    }

    #[test]
    fn list_padded_format_is_json() {
        empty_home(|| {
            let text = list_json(DiscoverArgs {
                format: Some(" json ".to_owned()),
                ..DiscoverArgs::default()
            })
            .expect("padded json");
            assert!(
                text.contains("\"skills\""),
                "spaces around json must still emit JSON: {text}"
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
            out.contains("<source>extra</source>"),
            "list XML must carry source like list JSON: {out}"
        );
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
            assert_eq!(
                resp["result"]["error_kind"], "unknown_skill",
                "hosts must branch without scraping Display: {resp}"
            );
            let text = call_text(&resp);
            assert_eq!(text, "unknown skill: no-such-skill", "{text}");
            assert!(!text.contains("skipped skill"), "{text}");
        });
    }

    #[test]
    fn skills_load_why_miss_peels_skill_miss_keys() {
        use craftbag::unknown_or_skipped_skill;

        // One table so MCP cannot keep hand-copying error_kind and drop a
        // new SkillMiss field the way CLI why --json already serializes.
        let unknown = unknown_or_skipped_skill("no-such-skill", &[]);
        let cases = [
            (
                60,
                "skills_load",
                json!({"name": "no-such-skill"}),
                &unknown,
            ),
            (61, "skills_why", json!({"name": "no-such-skill"}), &unknown),
        ];
        empty_home(|| {
            for (id, tool, args, miss) in cases {
                let peel = serde_json::to_value(miss).expect("skill miss serde");
                let peel_obj = peel.as_object().expect("object");
                assert!(
                    peel_obj.contains_key("error_kind") && peel_obj.contains_key("error"),
                    "SkillMiss peel is {{ error_kind, error }}: {peel}"
                );
                let resp = call(id, tool, args);
                assert_eq!(resp["result"]["isError"], true, "tool={tool} resp={resp}");
                for (key, value) in peel_obj {
                    assert_eq!(
                        &resp["result"][key], value,
                        "MCP {tool} must peel SkillMiss.{key} like CLI why --json: {resp}"
                    );
                }
                assert_eq!(
                    call_text(&resp),
                    miss.error,
                    "content[0].text stays the one-line miss: tool={tool}"
                );
                assert!(
                    resp["result"].get("path").is_none(),
                    "MCP {tool} unknown_skill omits path like CLI why --json: {resp}"
                );
            }
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
            assert_eq!(
                resp["result"]["error_kind"], "parse_error",
                "skipped load must reuse skip code, not Display: {resp}"
            );
            let text = call_text(&resp);
            assert_eq!(
                resp["result"]["error"], text,
                "parse skip must peel SkillMiss.error like CLI why --json: {resp}"
            );
            assert!(
                text.contains("skipped skill: Bad_Name"),
                "load must name the skipped skill: {text}"
            );
            assert!(text.contains("parse_error"), "{text}");
            assert!(
                !text.contains("unknown skill"),
                "skipped parse error must not look missing: {text}"
            );
            let path = resp["result"]["path"].as_str().expect("path");
            assert!(
                path.ends_with("Bad_Name/SKILL.md") || path.ends_with("Bad_Name\\SKILL.md"),
                "MCP load skip must peel SkillMiss.path next to isError: {resp}"
            );
        });
    }

    #[test]
    fn skills_load_present_null_user_invocable_peels_parse_error() {
        empty_home(|| {
            let tmp = tempfile::tempdir().expect("tmp");
            let pkg = tmp.path().join("demo");
            std::fs::create_dir_all(&pkg).expect("mkdir");
            std::fs::write(
                pkg.join("SKILL.md"),
                "---\nname: demo\ndescription: d\nuser_invocable: null\n---\nbody\n",
            )
            .expect("write");
            let parent = tmp.path().to_string_lossy().into_owned();
            let resp = call(
                88,
                "skills_load",
                json!({"name": "demo", "paths": [parent]}),
            );
            assert_eq!(resp["result"]["isError"], true);
            assert_eq!(
                resp["result"]["error_kind"], "parse_error",
                "present-null bool must reuse parse_error, not unknown_skill: {resp}"
            );
            let text = call_text(&resp);
            assert_eq!(
                resp["result"]["error"], text,
                "present-null must peel SkillMiss.error: {resp}"
            );
            assert!(
                text.contains("skipped skill: demo") && text.contains("parse_error"),
                "load must name the skipped package: {text}"
            );
            assert!(
                !text.contains("unknown skill"),
                "present-null must not look missing: {text}"
            );
            let path = resp["result"]["path"].as_str().expect("path");
            assert!(
                path.ends_with("demo/SKILL.md") || path.ends_with("demo\\SKILL.md"),
                "MCP load skip must peel SkillMiss.path: {resp}"
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
            assert_eq!(
                resp["result"]["error_kind"], "unknown_skill",
                "why miss must peel like load: {resp}"
            );
            let text = call_text(&resp);
            assert_eq!(text, "unknown skill: no-such-skill", "{text}");
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
    fn tools_list_load_why_describe_error_kind() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(52)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for name in ["skills_load", "skills_why"] {
            let tool = tools.iter().find(|t| t["name"] == name).expect(name);
            let desc = tool["description"].as_str().unwrap_or("");
            assert!(
                desc.contains("error_kind plus error"),
                "{name} must name SkillMiss.error as its own key, not a substring of error_kind: {desc}"
            );
        }
        let load = tools
            .iter()
            .find(|t| t["name"] == "skills_load")
            .expect("skills_load");
        let load_desc = load["description"].as_str().unwrap_or("");
        assert!(
            load_desc.contains("path when a skip"),
            "skills_load must name SkillMiss.path like CLI validate --json: {load_desc}"
        );
    }

    #[test]
    fn tools_list_describes_format_and_json_keys() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(54)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let list = tools
            .iter()
            .find(|t| t["name"] == "skills_list")
            .expect("skills_list");
        let desc = list["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("format") && desc.contains("{ skills, skips }"),
            "skills_list must name format and default json keys like CLI list --json: {desc}"
        );
    }

    #[test]
    fn tools_list_format_describes_watch_dirs_alias() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(53)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let list = tools
            .iter()
            .find(|t| t["name"] == "skills_list")
            .expect("skills_list");
        let desc = list["inputSchema"]["properties"]["format"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            desc.contains("watch-dirs") && desc.contains("watch_dirs"),
            "skills_list format must name live watch-dirs aliases: {desc}"
        );
    }

    #[test]
    fn tools_list_names_xml_available_skills() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(56)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let list = tools
            .iter()
            .find(|t| t["name"] == "skills_list")
            .expect("skills_list");
        let desc = list["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("<available_skills>"),
            "skills_list must name xml <available_skills> like json {{ skills, skips }}: {desc}"
        );
    }

    #[test]
    fn tools_list_format_names_json_skills_skips() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(55)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let list = tools
            .iter()
            .find(|t| t["name"] == "skills_list")
            .expect("skills_list");
        let desc = list["inputSchema"]["properties"]["format"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            desc.contains("{ skills, skips }"),
            "skills_list format json must name default keys like CLI list --json; xml already names <available_skills>: {desc}"
        );
    }

    #[test]
    fn tools_list_advertises_format_enum() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(50)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        let list = tools
            .iter()
            .find(|t| t["name"] == "skills_list")
            .expect("skills_list");
        let format = &list["inputSchema"]["properties"]["format"];
        assert_eq!(format["type"], "string", "{format}");
        let tokens: Vec<&str> = format["enum"]
            .as_array()
            .expect("enum")
            .iter()
            .map(|v| v.as_str().expect("tok"))
            .collect();
        assert_eq!(
            tokens,
            craftbag::ListFormat::CANONICAL_TOKENS,
            "skills_list format enum must match ListFormat::CANONICAL_TOKENS: {format}"
        );
        for alias in craftbag::ListFormat::ALIAS_TOKENS {
            assert!(
                !tokens.contains(alias),
                "schema enum is canonical tokens only; {alias} stays a parse alias: {format}"
            );
        }
    }

    #[test]
    fn tools_list_advertises_vendor_enum() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(51)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for tool in tools {
            let vendor = &tool["inputSchema"]["properties"]["vendor"];
            let tokens: Vec<&str> = vendor["items"]["enum"]
                .as_array()
                .expect("enum")
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert_eq!(
                tokens,
                craftbag::SkillSource::VENDOR_TOKENS,
                "{} vendor enum must match SkillSource::VENDOR_TOKENS: {vendor}",
                tool["name"]
            );
            let desc = vendor["description"].as_str().unwrap_or("");
            for example in json_array_examples(desc) {
                assert!(
                    tokens.contains(&example.as_str()),
                    "{} vendor description example {example:?} must be a schema enum token ({tokens:?}): {desc}",
                    tool["name"]
                );
            }
        }
    }

    /// JSON string arrays in help text (`["claude"]`). Prose aliases stay out.
    fn json_array_examples(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('[') {
            let after = &rest[start..];
            let Some(end) = after.find(']') else {
                break;
            };
            let slice = &after[..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(slice) {
                out.extend(arr);
            }
            rest = &after[end + 1..];
        }
        out
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

    #[test]
    fn tools_list_advertises_implicit_roots() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(141)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for tool in tools {
            let props = &tool["inputSchema"]["properties"];
            assert!(
                props.get("implicit_roots").is_some(),
                "{} must advertise implicit_roots like CLI --no-implicit-roots: {props}",
                tool["name"]
            );
            assert_eq!(
                props["implicit_roots"]["type"], "boolean",
                "{} implicit_roots type: {props}",
                tool["name"]
            );
            let desc = props["implicit_roots"]["description"]
                .as_str()
                .unwrap_or("");
            assert!(
                desc.contains("Omitted is true"),
                "{} implicit_roots schema must name omitted default true: {desc}",
                tool["name"]
            );
            assert!(
                desc.contains("cwd-to-git .agents"),
                "{} implicit_roots schema must attach .agents to cwd-to-git: {desc}",
                tool["name"]
            );
        }
    }

    #[test]
    fn skills_list_null_implicit_roots_is_error() {
        empty_home(|| {
            let resp = call(142, "skills_list", json!({"implicit_roots": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says implicit_roots is boolean; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog for null implicit_roots: {text}"
            );
        });
    }

    #[test]
    fn skills_list_implicit_roots_false_skips_home() {
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
        let listed = craftbag::with_home_override(Some(home.path().to_path_buf()), || {
            call(
                143,
                "skills_list",
                json!({
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false
                }),
            )
        });
        let text = call_text(&listed);
        assert!(
            text.contains("wanted"),
            "collection-only list must include extra wanted: {text}"
        );
        assert!(
            !text.contains("homeskill"),
            "collection-only list must omit HOME .agents: {text}"
        );
        let defaulted = craftbag::with_home_override(Some(home.path().to_path_buf()), || {
            call(
                144,
                "skills_list",
                json!({"paths": [extra.path().display().to_string()]}),
            )
        });
        let default_text = call_text(&defaulted);
        assert!(
            default_text.contains("homeskill"),
            "omitted implicit_roots must still load HOME .agents: {default_text}"
        );
    }

    #[test]
    fn tools_list_advertises_disabled() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(145)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for tool in tools {
            let props = &tool["inputSchema"]["properties"];
            assert!(
                props.get("disabled").is_some(),
                "{} must advertise disabled like CLI --disabled: {props}",
                tool["name"]
            );
            assert_eq!(
                props["disabled"]["type"], "array",
                "{} disabled type: {props}",
                tool["name"]
            );
            assert_eq!(
                props["disabled"]["items"]["type"], "string",
                "{} disabled items: {props}",
                tool["name"]
            );
        }
    }

    #[test]
    fn skills_list_null_disabled_is_error() {
        empty_home(|| {
            let resp = call(146, "skills_list", json!({"disabled": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says disabled is string array; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog for null disabled: {text}"
            );
        });
    }

    #[test]
    fn skills_list_disabled_omits_named_skill() {
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
        let listed = empty_home(|| {
            call(
                147,
                "skills_list",
                json!({
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false,
                    "disabled": ["OFF"]
                }),
            )
        });
        let text = call_text(&listed);
        assert!(
            text.contains("keep"),
            "disabled list must still include keep: {text}"
        );
        assert!(
            !text.contains("off"),
            "disabled OFF must NFKC-hide off: {text}"
        );
        let defaulted = empty_home(|| {
            call(
                148,
                "skills_list",
                json!({
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false
                }),
            )
        });
        let default_text = call_text(&defaulted);
        assert!(
            default_text.contains("off") && default_text.contains("keep"),
            "omitted disabled must still load off: {default_text}"
        );
    }

    #[test]
    fn skills_load_disabled_is_unknown() {
        let extra = tempfile::tempdir().expect("extra");
        let off = extra.path().join("off");
        fs::create_dir_all(&off).expect("off");
        fs::write(
            off.join("SKILL.md"),
            "---\nname: off\ndescription: hide\n---\nOFF\n",
        )
        .expect("write off");
        let resp = empty_home(|| {
            call(
                149,
                "skills_load",
                json!({
                    "name": "off",
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false,
                    "disabled": ["off"]
                }),
            )
        });
        assert_eq!(resp["result"]["isError"], true, "{}", call_text(&resp));
        assert_eq!(
            resp["result"]["error_kind"],
            "unknown_skill",
            "disabled load must be unknown, not a skip peel: {}",
            call_text(&resp)
        );
    }

    #[test]
    fn tools_list_advertises_ignore() {
        let names = handle(RpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(150)),
            method: Some("tools/list".into()),
            params: json!({}),
        })
        .expect("list");
        let tools = names["result"]["tools"].as_array().expect("tools");
        for tool in tools {
            let props = &tool["inputSchema"]["properties"];
            assert!(
                props.get("ignore").is_some(),
                "{} must advertise ignore like CLI --ignore: {props}",
                tool["name"]
            );
            assert_eq!(
                props["ignore"]["type"], "array",
                "{} ignore type: {props}",
                tool["name"]
            );
            assert_eq!(
                props["ignore"]["items"]["type"], "string",
                "{} ignore items: {props}",
                tool["name"]
            );
        }
    }

    #[test]
    fn skills_list_null_ignore_is_error() {
        empty_home(|| {
            let resp = call(151, "skills_list", json!({"ignore": null}));
            assert_eq!(
                resp["result"]["isError"],
                true,
                "schema says ignore is string array; null must not mean omitted: {}",
                call_text(&resp)
            );
            let text = call_text(&resp);
            assert!(
                text.contains("invalid type") || text.contains("expected"),
                "error must name the type mismatch: {text}"
            );
            assert!(
                !text.contains("\"skills\""),
                "must not return default catalog for null ignore: {text}"
            );
        });
    }

    #[test]
    fn skills_list_ignore_omits_prefix() {
        let extra = tempfile::tempdir().expect("extra");
        let keep = extra.path().join("keep");
        std::fs::create_dir_all(&keep).expect("keep");
        std::fs::write(
            keep.join("SKILL.md"),
            "---\nname: keep\ndescription: stay\n---\nKEEP\n",
        )
        .expect("write keep");
        let secret = extra.path().join("secret");
        std::fs::create_dir_all(&secret).expect("secret");
        std::fs::write(
            secret.join("SKILL.md"),
            "---\nname: secret\ndescription: hide\n---\nSECRET\n",
        )
        .expect("write secret");
        let listed = empty_home(|| {
            call(
                152,
                "skills_list",
                json!({
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false,
                    "ignore": [secret.display().to_string()]
                }),
            )
        });
        let text = call_text(&listed);
        assert!(
            text.contains("keep"),
            "ignore list must still include keep: {text}"
        );
        assert!(
            !text.contains("secret"),
            "ignored prefix must hide secret: {text}"
        );
        let defaulted = empty_home(|| {
            call(
                153,
                "skills_list",
                json!({
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false
                }),
            )
        });
        let default_text = call_text(&defaulted);
        assert!(
            default_text.contains("secret") && default_text.contains("keep"),
            "omitted ignore must still load secret: {default_text}"
        );
    }

    #[test]
    fn skills_load_ignore_is_unknown() {
        let extra = tempfile::tempdir().expect("extra");
        let secret = extra.path().join("secret");
        std::fs::create_dir_all(&secret).expect("secret");
        std::fs::write(
            secret.join("SKILL.md"),
            "---\nname: secret\ndescription: hide\n---\nSECRET\n",
        )
        .expect("write secret");
        let resp = empty_home(|| {
            call(
                154,
                "skills_load",
                json!({
                    "name": "secret",
                    "paths": [extra.path().display().to_string()],
                    "implicit_roots": false,
                    "ignore": [secret.display().to_string()]
                }),
            )
        });
        assert_eq!(resp["result"]["isError"], true, "{}", call_text(&resp));
        assert_eq!(
            resp["result"]["error_kind"],
            "unknown_skill",
            "ignored load must be unknown, not a skip peel: {}",
            call_text(&resp)
        );
    }
}
