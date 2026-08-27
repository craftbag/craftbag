//! Parsed skill package. Host-neutral; no required Bline types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::source::SkillSource;

/// agentskills.io: name max length.
pub const SKILL_NAME_MAX_CHARS: usize = 64;
/// agentskills.io: description max length.
pub const SKILL_DESCRIPTION_MAX_CHARS: usize = 1024;
/// agentskills.io: compatibility max length.
pub const SKILL_COMPATIBILITY_MAX_CHARS: usize = 500;
/// Soft warn threshold: agentskills recommends keeping SKILL.md under ~500 lines.
pub const SKILL_BODY_LINE_SOFT_WARN: usize = 500;
/// Hard cap on SKILL.md bytes for discover and validate. Prevents unbounded reads.
pub const SKILL_MD_MAX_BYTES: u64 = 1_048_576;

/// A parsed skill loaded from a SKILL.md file.
///
/// Required/optional frontmatter follow
/// [agentskills.io](https://agentskills.io/specification). Host extensions
/// (`triggers`, `user_invocable`, `disable_model_invocation`, …) are
/// documented as non-spec fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Human-readable skill name (from frontmatter). agentskills: required.
    pub name: String,
    /// Short description of what the skill provides (from frontmatter).
    /// agentskills: required, max 1024 characters.
    pub description: String,
    /// Trigger phrases that activate this skill (host extension). Empty
    /// means always-active unless
    /// [`SkillSource::empty_triggers_not_always_active`] (compat vendors
    /// `claude` / `cursor` / `grok`).
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The Markdown body content injected into the prompt.
    #[serde(default)]
    pub content: String,
    /// Where the skill was discovered.
    ///
    /// Serialize stays the lib enum (`extraPath`, `{vendor:{name}}`).
    /// Deserialize also accepts list/why wire tokens (`extra`, `claude`)
    /// so a host row is not rejected.
    #[serde(deserialize_with = "SkillSource::deserialize_host")]
    pub source: SkillSource,
    /// Path to the SKILL.md file (set during discovery, None for in-memory skills).
    ///
    /// Serialize stays omitted. Deserialize accepts list/why `path` so a
    /// host row is not silently path-less.
    #[serde(default, skip_serializing, alias = "path")]
    pub source_path: Option<PathBuf>,
    /// agentskills optional: license name or license file reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// agentskills optional: environment requirements (max 500 chars).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    /// agentskills optional: arbitrary string map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    /// agentskills optional experimental: space-separated pre-approved tools.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "allowed_tools"
    )]
    pub allowed_tools: Option<String>,
    /// Host extension: show in slash palette / bare `/name`. Default true.
    #[serde(default = "default_true", alias = "user_invocable")]
    pub user_invocable: bool,
    /// Host extension: never auto-inject full body; slash only. Default false.
    #[serde(default, alias = "disable_model_invocation")]
    pub disable_model_invocation: bool,
    /// Host extension: palette argument hint.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "argument_hint"
    )]
    pub argument_hint: Option<String>,
    /// Host extension: extra when-to-use text for ranking/catalog.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "when_to_use"
    )]
    pub when_to_use: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Skill {
    /// Build a skill with agentskills-required fields and safe defaults.
    ///
    /// `source` defaults to [`SkillSource::Agents`]. Callers override it
    /// from the discovery root.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            triggers: Vec::new(),
            content: content.into(),
            source: SkillSource::Agents,
            source_path: None,
            license: None,
            compatibility: None,
            metadata: BTreeMap::new(),
            allowed_tools: None,
            user_invocable: true,
            disable_model_invocation: false,
            argument_hint: None,
            when_to_use: None,
        }
    }

    /// Parent directory of `SKILL.md` (package root), if known.
    pub fn package_root(&self) -> Option<&Path> {
        self.source_path.as_ref().and_then(|p| p.parent())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::{
        SKILL_BODY_LINE_SOFT_WARN, SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS,
        SKILL_MD_MAX_BYTES, SKILL_NAME_MAX_CHARS, Skill,
    };
    use crate::source::SkillSource;

    #[test]
    fn parse_max_constants_match_bline() {
        assert_eq!(SKILL_NAME_MAX_CHARS, 64);
        assert_eq!(SKILL_DESCRIPTION_MAX_CHARS, 1024);
        assert_eq!(SKILL_COMPATIBILITY_MAX_CHARS, 500);
        assert_eq!(SKILL_BODY_LINE_SOFT_WARN, 500);
        assert_eq!(SKILL_MD_MAX_BYTES, 1_048_576);
    }

    #[test]
    fn new_defaults_to_agents_source() {
        let skill = Skill::new("demo", "A demo skill", "## Body\n");
        assert_eq!(skill.source, SkillSource::Agents);
        assert!(skill.user_invocable);
        assert!(!skill.disable_model_invocation);
        assert!(skill.triggers.is_empty());
        assert!(skill.package_root().is_none());
    }

    #[test]
    fn package_root_is_parent_of_skill_md() {
        let mut skill = Skill::new("demo", "A demo skill", "");
        skill.source_path = Some(PathBuf::from("/tmp/demo/SKILL.md"));
        assert_eq!(
            skill.package_root(),
            Some(std::path::Path::new("/tmp/demo"))
        );
    }

    #[test]
    fn serde_round_trip_camel_case() {
        let mut skill = Skill::new("test-skill", "A test skill", "## Usage\nDo things.");
        skill.triggers = vec!["rust".to_owned(), "cargo".to_owned()];
        skill.source = SkillSource::User;
        skill.license = Some("Apache-2.0".to_owned());
        skill.when_to_use = Some("when testing serde".to_owned());
        skill.source_path = Some(PathBuf::from("/tmp/test-skill/SKILL.md"));

        let json = serde_json::to_string(&skill).expect("serialize");
        assert!(json.contains("\"userInvocable\""), "json={json}");
        assert!(json.contains("\"disableModelInvocation\""), "json={json}");
        assert!(json.contains("\"whenToUse\""), "json={json}");
        assert!(
            !json.contains("sourcePath") && !json.contains("source_path"),
            "source_path must be skipped: {json}"
        );

        let back: Skill = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, skill.name);
        assert_eq!(back.description, skill.description);
        assert_eq!(back.triggers, skill.triggers);
        assert_eq!(back.content, skill.content);
        assert_eq!(back.source, skill.source);
        assert_eq!(back.license, skill.license);
        assert_eq!(back.when_to_use, skill.when_to_use);
        assert!(
            back.source_path.is_none(),
            "skipped source_path must not round-trip"
        );
    }

    #[test]
    fn missing_user_invocable_defaults_true() {
        let json = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "agents"
        }"#;
        let skill: Skill = serde_json::from_str(json).expect("de");
        assert!(skill.user_invocable);
        assert!(!skill.disable_model_invocation);
        assert_eq!(skill.source, SkillSource::Agents);
    }

    #[test]
    fn serde_accepts_snake_case_list_why_keys() {
        // List / why JSON uses these keys. Skill camelCase serde must
        // still honor them so a host row is not silently defaulted.
        let json = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "agents",
            "user_invocable": false,
            "disable_model_invocation": true,
            "argument_hint": "[name]",
            "when_to_use": "after rebase",
            "allowed_tools": "Read"
        }"#;
        let skill: Skill = serde_json::from_str(json).expect("snake_case Skill JSON");
        assert!(
            !skill.user_invocable,
            "user_invocable: false must not stay the omitted-flag default"
        );
        assert!(skill.disable_model_invocation);
        assert_eq!(skill.argument_hint.as_deref(), Some("[name]"));
        assert_eq!(skill.when_to_use.as_deref(), Some("after rebase"));
        assert_eq!(skill.allowed_tools.as_deref(), Some("Read"));
        let out = serde_json::to_string(&skill).expect("ser");
        assert!(
            out.contains("\"userInvocable\""),
            "Skill serialize stays camelCase: {out}"
        );
        assert!(
            !out.contains("\"user_invocable\""),
            "Skill serialize must not switch to snake_case: {out}"
        );
    }

    #[test]
    fn serde_accepts_list_why_source_tokens() {
        // List / why JSON uses host wire tokens (`extra`, `claude`).
        // Skill enum serde is still `extraPath` / `{vendor:{name}}`.
        // A host that feeds a list/why row back into Skill must not
        // reject the wire token (PR 150 leftover on `source`).
        let extra = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "extra"
        }"#;
        let skill: Skill = serde_json::from_str(extra).expect("list/why extra source");
        assert_eq!(skill.source, SkillSource::ExtraPath);
        let snake = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "extra_path"
        }"#;
        let skill: Skill = serde_json::from_str(snake).expect("list/why extra_path source");
        assert_eq!(skill.source, SkillSource::ExtraPath);
        let vendor = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "claude"
        }"#;
        let skill: Skill = serde_json::from_str(vendor).expect("list/why vendor source");
        assert_eq!(
            skill.source,
            SkillSource::Vendor {
                name: "claude".to_owned()
            }
        );
        let old_extra: Skill = serde_json::from_str(
            r#"{"name":"x","description":"y","content":"z","source":"extraPath"}"#,
        )
        .expect("old Skill extraPath");
        assert_eq!(old_extra.source, SkillSource::ExtraPath);
        let old_vendor: Skill = serde_json::from_str(
            r#"{"name":"x","description":"y","content":"z","source":{"vendor":{"name":"claude"}}}"#,
        )
        .expect("old Skill vendor object");
        assert_eq!(
            old_vendor.source,
            SkillSource::Vendor {
                name: "claude".to_owned()
            }
        );
        let out = serde_json::to_string(&old_extra).expect("ser extra");
        assert!(
            out.contains("\"extraPath\""),
            "Skill serialize stays extraPath, not the list/why extra token: {out}"
        );
        assert!(
            !out.contains("\"extra\""),
            "Skill serialize must not switch ExtraPath to extra: {out}"
        );
        let out = serde_json::to_string(&old_vendor).expect("ser vendor");
        assert!(
            out.contains("\"vendor\"") && out.contains("\"claude\""),
            "Skill serialize stays externally tagged vendor: {out}"
        );
        assert!(
            !out.contains("\"source\":\"claude\""),
            "Skill serialize must not switch vendor to the list/why string: {out}"
        );
    }

    #[test]
    fn serde_accepts_list_why_path() {
        // List / why JSON uses `path`. Skill `source_path` is omitted on
        // serialize. A host that feeds a list/why row back into Skill
        // must not stay path-less (PR 203 leftover on `path`).
        let json = r#"{
            "name": "x",
            "description": "y",
            "content": "z",
            "source": "extra",
            "path": "/tmp/x/SKILL.md"
        }"#;
        let skill: Skill = serde_json::from_str(json).expect("list/why path");
        assert_eq!(
            skill.source_path.as_deref(),
            Some(std::path::Path::new("/tmp/x/SKILL.md")),
            "list/why path must populate source_path, not stay skipped None"
        );
        assert_eq!(
            skill.package_root(),
            Some(std::path::Path::new("/tmp/x")),
            "list/why path must keep package_root for the load envelope"
        );
        let out = serde_json::to_string(&skill).expect("ser");
        assert!(
            !out.contains("sourcePath")
                && !out.contains("source_path")
                && !out.contains("\"path\""),
            "Skill serialize must still omit source_path: {out}"
        );
    }

    #[test]
    fn serde_accepts_list_why_row_without_content() {
        // List / why JSON is SkillSummary: name, description, source, path,
        // flags. It never emits `content`. PRs 203/213 accepted source tokens
        // and `path` on a row that still faked `"content":"z"`. A host that
        // feeds a real list/why row back into Skill must not reject it.
        let json = r#"{
            "name": "x",
            "description": "y",
            "source": "extra",
            "path": "/tmp/x/SKILL.md",
            "user_invocable": false,
            "disable_model_invocation": true,
            "argument_hint": "[name]",
            "when_to_use": "after rebase",
            "triggers": ["git"],
            "allowed_tools": "Read",
            "license": "MIT",
            "compatibility": "rust",
            "metadata": {"author": "A"}
        }"#;
        let skill: Skill = serde_json::from_str(json).expect("list/why row without content");
        assert_eq!(
            skill.content, "",
            "omitted list/why content must default empty, not reject the row"
        );
        assert_eq!(skill.name, "x");
        assert_eq!(skill.description, "y");
        assert_eq!(skill.source, SkillSource::ExtraPath);
        assert_eq!(
            skill.source_path.as_deref(),
            Some(std::path::Path::new("/tmp/x/SKILL.md"))
        );
        assert!(!skill.user_invocable);
        assert!(skill.disable_model_invocation);
        assert_eq!(skill.argument_hint.as_deref(), Some("[name]"));
        assert_eq!(skill.when_to_use.as_deref(), Some("after rebase"));
        assert_eq!(skill.triggers, vec!["git".to_owned()]);
        assert_eq!(skill.allowed_tools.as_deref(), Some("Read"));
        assert_eq!(skill.license.as_deref(), Some("MIT"));
        assert_eq!(skill.compatibility.as_deref(), Some("rust"));
        assert_eq!(skill.metadata.get("author").map(String::as_str), Some("A"));
        assert_eq!(
            skill.package_root(),
            Some(std::path::Path::new("/tmp/x")),
            "list/why path must still keep package_root when content is omitted"
        );
        let out = serde_json::to_string(&skill).expect("ser");
        assert!(
            out.contains("\"content\":\"\""),
            "Skill serialize still emits content (empty), not omit: {out}"
        );
        assert!(
            !out.contains("sourcePath")
                && !out.contains("source_path")
                && !out.contains("\"path\""),
            "Skill serialize must still omit source_path: {out}"
        );
    }

    /// List/why `path` must populate `source_path` for any host string, not
    /// only `/tmp/x/SKILL.md`. Includes empty, unicode, and a segment named
    /// `source_path` (a `contains("source_path")` omit check would false-fail).
    fn host_path() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("/tmp/x/SKILL.md".to_owned()),
            Just("/tmp/winner_path/source_path/SKILL.md".to_owned()),
            Just("/home/user/.agents/skills/foo/SKILL.md".to_owned()),
            Just("/tmp/ünicode dir/技能/SKILL.md".to_owned()),
            Just(r"C:\Users\foo\SKILL.md".to_owned()),
            ".{1,64}",
            prop::collection::vec("[A-Za-z0-9._-]{1,10}", 1usize..5)
                .prop_map(|parts| format!("/{}/SKILL.md", parts.join("/"))),
        ]
    }

    fn list_why_source_token() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec![
            "extra",
            "extra_path",
            "extraPath",
            "agents",
            "user",
            "claude",
            "config",
        ])
    }

    proptest! {
        #[test]
        fn serde_list_why_path_for_any_host_path(
            path in host_path(),
            source in list_why_source_token(),
        ) {
            let json = serde_json::json!({
                "name": "x",
                "description": "y",
                "content": "z",
                "source": source,
                "path": path,
            })
            .to_string();
            let skill: Skill = serde_json::from_str(&json).expect("list/why path");
            prop_assert_eq!(
                skill.source_path.as_deref(),
                Some(std::path::Path::new(&path))
            );
            prop_assert_eq!(skill.package_root(), std::path::Path::new(&path).parent());
            let out = serde_json::to_value(&skill).expect("ser");
            prop_assert!(
                out.get("sourcePath").is_none()
                    && out.get("source_path").is_none()
                    && out.get("path").is_none(),
                "Skill serialize must still omit source_path: {out}"
            );
        }
    }
}
