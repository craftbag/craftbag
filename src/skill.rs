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
    /// Trigger phrases that activate this skill (host extension). Empty means
    /// always active for non-vendor sources.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The Markdown body content injected into the prompt.
    pub content: String,
    /// Where the skill was discovered.
    pub source: SkillSource,
    /// Path to the SKILL.md file (set during discovery, None for in-memory skills).
    #[serde(skip)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    /// Host extension: show in slash palette / bare `/name`. Default true.
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    /// Host extension: never auto-inject full body; slash only. Default false.
    #[serde(default)]
    pub disable_model_invocation: bool,
    /// Host extension: palette argument hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    /// Host extension: extra when-to-use text for ranking/catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
}
