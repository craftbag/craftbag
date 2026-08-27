//! Doctor: why a skill loaded, skipped, or did not auto-inject.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::activate::{ProgressiveBudgets, filter_skills, progressive_budgets, trigger_matches};
use crate::discover::{SkillMiss, unknown_or_skipped_skill};
use crate::skill::Skill;
use crate::skip::{DiscoveryReport, SkillSkip};
use crate::source::SkillSource;

/// Why a loaded skill was or was not auto-injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationReason {
    Injected,
    DisableModelInvocation,
    VendorEmptyTriggers,
    NoTriggerMatch,
    BudgetOmitted,
}

impl ActivationReason {
    /// Wire name (`snake_case`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Injected => "injected",
            Self::DisableModelInvocation => "disable_model_invocation",
            Self::VendorEmptyTriggers => "vendor_empty_triggers",
            Self::NoTriggerMatch => "no_trigger_match",
            Self::BudgetOmitted => "budget_omitted",
        }
    }
}

/// One activation decision for `why`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationDecision {
    pub name: String,
    pub reason: ActivationReason,
    pub detail: String,
}

impl Serialize for ActivationDecision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            name: &'a str,
            reason: ActivationReason,
            code: &'a str,
            detail: &'a str,
        }
        Wire {
            name: &self.name,
            reason: self.reason,
            code: self.reason.as_str(),
            detail: &self.detail,
        }
        .serialize(serializer)
    }
}

impl ActivationDecision {
    /// Stable machine code (`reason.as_str()`).
    pub fn code(&self) -> &'static str {
        self.reason.as_str()
    }
}

/// Loaded skill row for `why`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub name: String,
    /// Same key as list JSON / list XML. Omitted on cached pre-this-PR
    /// why JSON (empty string).
    #[serde(default)]
    pub description: String,
    #[serde(
        serialize_with = "SkillSource::serialize_wire",
        deserialize_with = "SkillSource::deserialize_host"
    )]
    pub source: SkillSource,
    pub path: Option<PathBuf>,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    #[serde(rename = "user_invocable", default = "default_user_invocable")]
    pub user_invocable: bool,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    #[serde(rename = "disable_model_invocation", default)]
    pub disable_model_invocation: bool,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    /// Omitted on cached pre-this-PR why JSON (`None`).
    #[serde(rename = "argument_hint", default)]
    pub argument_hint: Option<String>,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    /// Omitted on cached pre-this-PR why JSON (`None`).
    #[serde(rename = "when_to_use", default)]
    pub when_to_use: Option<String>,
    /// Same snake_case key as list JSON / list XML. Host-extension trigger
    /// phrases used by `filter_skills` / `why.activation`. Empty on cached
    /// pre-this-PR why JSON.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    /// Official agentskills `allowed-tools`. Omitted on cached pre-this-PR
    /// why JSON (`None`).
    #[serde(rename = "allowed_tools", default)]
    pub allowed_tools: Option<String>,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    /// Official agentskills `license`. Omitted on cached pre-this-PR
    /// why JSON (`None`).
    #[serde(default)]
    pub license: Option<String>,
    /// Same snake_case key as list JSON / list XML (not Skill camelCase).
    /// Official agentskills `compatibility`. Omitted on cached pre-this-PR
    /// why JSON (`None`).
    #[serde(default)]
    pub compatibility: Option<String>,
    /// Same snake_case key as list JSON / list XML. Official agentskills
    /// `metadata` map. Empty object `{}` is always serialized (not omitted).
    /// Empty on cached pre-this-PR why JSON.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl From<&Skill> for SkillSummary {
    fn from(skill: &Skill) -> Self {
        Self {
            name: skill.name.clone(),
            description: skill.description.clone(),
            source: skill.source.clone(),
            path: skill.source_path.clone(),
            user_invocable: skill.user_invocable,
            disable_model_invocation: skill.disable_model_invocation,
            argument_hint: skill.argument_hint.clone(),
            when_to_use: skill.when_to_use.clone(),
            triggers: skill.triggers.clone(),
            allowed_tools: skill.allowed_tools.clone(),
            license: skill.license.clone(),
            compatibility: skill.compatibility.clone(),
            metadata: skill.metadata.clone(),
        }
    }
}

fn default_user_invocable() -> bool {
    true
}

/// Doctor report: loaded, skipped, and activation decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyReport {
    pub loaded: Vec<SkillSummary>,
    pub skips: Vec<SkillSkip>,
    pub activation: Vec<ActivationDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

impl WhyReport {
    /// Message when a name query matched neither a loaded skill nor a skip.
    ///
    /// Omitted name is not a query. Whitespace-only name is a query and
    /// is unknown. The echoed name uses [`crate::sanitize_error_token`]
    /// so CLI/MCP stderr stays one line, same as load.
    pub fn unknown_skill_message(&self) -> Option<String> {
        self.unknown_skill_miss().map(|m| m.error)
    }

    /// Same miss as [`Self::unknown_skill_message`], with `error_kind`.
    pub fn unknown_skill_miss(&self) -> Option<SkillMiss> {
        let want = self.query.as_deref()?;
        if !self.loaded.is_empty() {
            return None;
        }
        if self.skips.is_empty() {
            return Some(unknown_or_skipped_skill(want, &[]));
        }
        // Host-token refuse skips are not a package identity. Peel them
        // so a named why is unreadable + path, not a silent unknown.
        if self.skips.iter().all(|s| s.is_host_token_refuse()) {
            return Some(unknown_or_skipped_skill(want, &self.skips));
        }
        None
    }
}

/// Text why rows for CLI `why` (not `--json`) and MCP `skills_why`
/// `format=text`: `loaded\tname\tpath`, skip TSV, then
/// `activation\tname\treason\tdetail`.
///
/// Path and detail go through [`crate::sanitize_error_token`] so a
/// leftover implicit package cannot split the row (U+2028). Skip
/// rows share [`crate::format_skip_tsv`].
pub fn format_why_text(report: &WhyReport) -> String {
    let mut out = String::new();
    for skill in &report.loaded {
        let path = skill
            .path
            .as_ref()
            .map(|p| crate::sanitize_error_token(&p.display().to_string()))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "loaded\t{}\t{}",
            crate::sanitize_error_token(&skill.name),
            path
        );
    }
    out.push_str(&crate::format_skip_tsv(&report.skips));
    for decision in &report.activation {
        let _ = writeln!(
            out,
            "activation\t{}\t{}\t{}",
            crate::sanitize_error_token(&decision.name),
            decision.reason.as_str(),
            crate::sanitize_error_token(&decision.detail)
        );
    }
    out
}

/// Explain loaded vs skipped skills and optional activation decisions.
///
/// Loaded rows include `description`, `user_invocable`,
/// `disable_model_invocation`, `argument_hint`, `when_to_use`,
/// `triggers`, `allowed_tools`, `license`, `compatibility`, and
/// `metadata` (same keys as list JSON / list XML).
/// Does not take [`crate::DiscoveryOptions`], so disabled-by-name and
/// vendor denylist are not activation reasons.
pub fn why(
    report: &DiscoveryReport,
    query: Option<&str>,
    context: Option<&str>,
    budgets: Option<ProgressiveBudgets>,
) -> WhyReport {
    // Trim for matching. Keep Some("") when the caller passed only
    // whitespace so that is an unknown name, not an omitted filter.
    let q = query.map(str::trim);
    let loaded: Vec<SkillSummary> = report
        .skills
        .iter()
        .filter(|s| name_matches(q, &s.name))
        .map(SkillSummary::from)
        .collect();
    let skips: Vec<SkillSkip> = report
        .skips
        .iter()
        .filter(|s| match q {
            None => true,
            Some(want) => s.matches_requested_name(want) || s.is_host_token_refuse(),
        })
        .cloned()
        .collect();

    let activation = match context {
        Some(ctx) => activation_decisions(&report.skills, q, ctx, budgets),
        None => Vec::new(),
    };

    WhyReport {
        loaded,
        skips,
        activation,
        query: query.map(ToOwned::to_owned),
    }
}

fn name_matches(query: Option<&str>, name: &str) -> bool {
    match query {
        None => true,
        // Same identity as load / skip matching: NFKC then Unicode case fold.
        Some(want) => crate::parse::skill_names_equal(name, want),
    }
}

fn activation_decisions(
    skills: &[Skill],
    query: Option<&str>,
    context: &str,
    budgets: Option<ProgressiveBudgets>,
) -> Vec<ActivationDecision> {
    let body_budget = budgets
        .unwrap_or_else(|| progressive_budgets(8_000))
        .body_token_budget;
    let injected = filter_skills(skills, context, body_budget);
    let context_lower = context.to_lowercase();
    let mut out = Vec::new();
    for skill in skills {
        if !name_matches(query, &skill.name) {
            continue;
        }
        out.push(decide_one(skill, &injected, &context_lower, body_budget));
    }
    out
}

fn decide_one(
    skill: &Skill,
    injected: &[&Skill],
    context_lower: &str,
    _body_budget: usize,
) -> ActivationDecision {
    if skill.disable_model_invocation {
        return ActivationDecision {
            name: skill.name.clone(),
            reason: ActivationReason::DisableModelInvocation,
            detail: "disable_model_invocation is set".to_owned(),
        };
    }
    if skill.triggers.is_empty() && skill.source.empty_triggers_not_always_active() {
        return ActivationDecision {
            name: skill.name.clone(),
            reason: ActivationReason::VendorEmptyTriggers,
            detail: "vendor source with empty triggers is not always-active".to_owned(),
        };
    }
    if !skill.triggers.is_empty() {
        let hit = skill
            .triggers
            .iter()
            .any(|t| trigger_matches(context_lower, t));
        if !hit {
            return ActivationDecision {
                name: skill.name.clone(),
                reason: ActivationReason::NoTriggerMatch,
                detail: "no trigger matched the context".to_owned(),
            };
        }
    }
    if injected.iter().any(|s| s.name == skill.name) {
        return ActivationDecision {
            name: skill.name.clone(),
            reason: ActivationReason::Injected,
            detail: "selected for auto-inject".to_owned(),
        };
    }
    ActivationDecision {
        name: skill.name.clone(),
        reason: ActivationReason::BudgetOmitted,
        detail: "matched but omitted by the body token budget".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivationReason, SkillSummary, WhyReport, why};
    use crate::skill::Skill;
    use crate::skip::{DiscoveryReport, SkillSkip, SkipKind};
    use crate::source::SkillSource;
    use std::path::PathBuf;

    #[test]
    fn why_reports_collision_winner_path() {
        let skill = Skill::new("foo", "d", "body");
        let skip = SkillSkip {
            path: PathBuf::from("/b/foo/SKILL.md"),
            name: Some("foo".to_owned()),
            kind: SkipKind::NameCollision,
            detail: "lost".to_owned(),
            winner_path: Some(PathBuf::from("/a/foo/SKILL.md")),
        };
        let report = DiscoveryReport {
            skills: vec![skill],
            skips: vec![skip],
        };
        let why = why(&report, Some("foo"), None, None);
        assert_eq!(why.loaded.len(), 1);
        assert_eq!(why.skips.len(), 1);
        assert_eq!(
            why.skips[0].winner_path.as_deref(),
            Some(std::path::Path::new("/a/foo/SKILL.md")),
            "why must keep the collision winner path, not only Some(_)"
        );
        assert!(why.activation.is_empty());
    }

    #[test]
    fn why_nameless_parse_skip_matches_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![skip],
        };
        let why = why(&report, Some("DEMO"), None, None);
        assert_eq!(
            why.skips.len(),
            1,
            "why must find nameless parse skip by package dir"
        );
        assert_eq!(why.skips[0].kind, SkipKind::ParseError);
        assert!(
            why.unknown_skill_message().is_none(),
            "nameless parse skip is not unknown"
        );
    }

    #[test]
    fn why_nameless_unreadable_skip_matches_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/skill.md"),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "Permission denied (os error 13)".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![skip],
        };
        let why = why(&report, Some("demo"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert_eq!(why.skips[0].kind, SkipKind::Unreadable);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn why_nameless_root_file_skip_stays_unknown() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills/SKILL.md"),
            name: None,
            kind: SkipKind::RootFile,
            detail: "put the file in a named subdirectory.".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![skip],
        };
        let why = why(&report, Some("skills"), None, None);
        assert!(
            why.skips.is_empty(),
            "root-file skip is not a named package"
        );
        assert_eq!(
            why.unknown_skill_message().as_deref(),
            Some("unknown skill: skills")
        );
    }

    #[test]
    fn why_named_root_file_skip_is_not_unknown() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills/SKILL.md"),
            name: Some("loose".to_owned()),
            kind: SkipKind::RootFile,
            detail: "put the file in a named subdirectory.".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![skip],
        };
        let why = why(&report, Some("loose"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert_eq!(why.skips[0].kind, SkipKind::RootFile);
        assert!(
            why.unknown_skill_message().is_none(),
            "root_file skip with a frontmatter name is not unknown"
        );
        let by_dir = super::why(&report, Some("skills"), None, None);
        assert!(by_dir.skips.is_empty());
        assert_eq!(
            by_dir.unknown_skill_message().as_deref(),
            Some("unknown skill: skills")
        );
    }

    #[test]
    fn why_unknown_name_is_unknown() {
        let report = DiscoveryReport::default();
        let why = why(&report, Some("no-such"), None, None);
        assert_eq!(
            why.unknown_skill_message().as_deref(),
            Some("unknown skill: no-such")
        );
        let miss = why.unknown_skill_miss().expect("unknown");
        assert_eq!(miss.error_kind, "unknown_skill");
        assert_eq!(miss.error, "unknown skill: no-such");
        assert!(miss.is_not_found());
        let json = serde_json::to_string(&miss).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(v["error_kind"], "unknown_skill", "json={json}");
        assert_eq!(v["error"], "unknown skill: no-such", "json={json}");
        assert!(
            v.get("errorKind").is_none(),
            "error_kind must stay snake_case: {json}"
        );
    }

    #[test]
    fn why_unknown_skill_message_stays_one_line() {
        let report = DiscoveryReport::default();
        let why = why(&report, Some("no\nsuch"), None, None);
        let msg = why.unknown_skill_message().expect("unknown");
        assert_eq!(
            msg, "unknown skill: no?such",
            "why must sanitize like load so CLI/MCP stderr stays one line"
        );
        assert_eq!(msg.lines().count(), 1, "msg={msg:?}");
        let why = super::why(&report, Some("no\u{2028}such"), None, None);
        let msg = why.unknown_skill_message().expect("unknown");
        assert_eq!(msg, "unknown skill: no?such", "msg={msg:?}");
        assert!(!msg.contains('\u{2028}'), "msg={msg:?}");
        let why = super::why(&report, Some("no\u{2029}such"), None, None);
        let msg = why.unknown_skill_message().expect("unknown");
        assert_eq!(msg, "unknown skill: no?such", "msg={msg:?}");
    }

    #[test]
    fn why_unicode_query_matches_nfkc_loaded_name() {
        let skill = Skill::new("перевод", "d", "body");
        let report = DiscoveryReport {
            skills: vec![skill],
            skips: vec![],
        };
        let folded = why(&report, Some("ПЕРЕВОД"), None, None);
        assert_eq!(
            folded.loaded.len(),
            1,
            "why must NFKC-fold Cyrillic like load, not ASCII-only: {:?}",
            folded.loaded
        );
        assert_eq!(folded.loaded[0].name, "перевод");
        assert!(folded.unknown_skill_message().is_none());

        let nfc = Skill::new("café", "d", "body");
        let nfc_report = DiscoveryReport {
            skills: vec![nfc],
            skips: vec![],
        };
        let nfd = why(&nfc_report, Some("cafe\u{0301}"), None, None);
        assert_eq!(
            nfd.loaded.len(),
            1,
            "why must treat NFD query as the NFC package: {:?}",
            nfd.loaded
        );
        assert_eq!(nfd.loaded[0].name, "café");
        assert!(nfd.unknown_skill_message().is_none());
    }

    #[test]
    fn why_whitespace_query_is_unknown_not_unfiltered() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![skip],
        };
        let filtered = why(&report, Some("   "), None, None);
        assert!(
            filtered.skips.is_empty(),
            "whitespace is an explicit name, not omit-filter: {:?}",
            filtered.skips
        );
        assert_eq!(
            filtered.unknown_skill_message().as_deref(),
            Some("unknown skill:    "),
            "why must agree with load that a whitespace name is unknown"
        );
        let omitted = why(&report, None, None, None);
        assert_eq!(
            omitted.skips.len(),
            1,
            "omitted name still lists every skip"
        );
        assert!(omitted.unknown_skill_message().is_none());
    }

    #[test]
    fn why_named_and_nameless_collision_keeps_both() {
        let named = SkillSkip {
            path: PathBuf::from("/tmp/other/SKILL.md"),
            name: Some("alpha".to_owned()),
            kind: SkipKind::ParseError,
            detail: "invalid YAML".to_owned(),
            winner_path: None,
        };
        let nameless = SkillSkip {
            path: PathBuf::from("/tmp/alpha/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        let report = DiscoveryReport {
            skills: vec![],
            skips: vec![named, nameless],
        };
        let by_name = why(&report, Some("alpha"), None, None);
        assert_eq!(by_name.skips.len(), 2, "both skips identify as alpha");
        assert!(by_name.unknown_skill_message().is_none());
        let by_dir = why(&report, Some("other"), None, None);
        assert_eq!(
            by_dir.skips.len(),
            1,
            "only the named skip lives in other/: {:?}",
            by_dir.skips
        );
        assert_eq!(by_dir.skips[0].name.as_deref(), Some("alpha"));
        assert!(by_dir.unknown_skill_message().is_none());
    }

    #[test]
    fn why_activation_matches_filter_skills_for_every_vendor() {
        use crate::activate::filter_skills;

        for name in SkillSource::VENDOR_TOKENS {
            let mut skill = Skill::new("legacy", "d", "x".repeat(80));
            skill.source = SkillSource::Vendor {
                name: (*name).to_owned(),
            };
            let report = DiscoveryReport {
                skills: vec![skill.clone()],
                skips: vec![],
            };
            let why = why(&report, None, Some("hello"), None);
            let injected = filter_skills(&report.skills, "hello", 10_000);
            assert_eq!(why.activation.len(), 1, "vendor {name}");
            if skill.source.empty_triggers_not_always_active() {
                assert!(
                    injected.is_empty(),
                    "filter_skills must omit empty-trigger {name}"
                );
                assert_eq!(
                    why.activation[0].reason,
                    ActivationReason::VendorEmptyTriggers,
                    "why must not inject empty-trigger {name}"
                );
            } else {
                assert_eq!(
                    injected.len(),
                    1,
                    "filter_skills must inject empty-trigger {name}"
                );
                assert_eq!(
                    why.activation[0].reason,
                    ActivationReason::Injected,
                    "why must inject empty-trigger {name}"
                );
            }
        }
    }

    #[test]
    fn why_activation_vendor_empty_and_budget() {
        let mut always = Skill::new("a-small", "d", "x".repeat(80));
        always.source = SkillSource::Agents;
        let mut huge = Skill::new("b-huge", "d", "x".repeat(2_000));
        huge.source = SkillSource::Agents;
        let mut vendor = Skill::new("personal", "d", "x".repeat(40));
        vendor.source = SkillSource::Vendor {
            name: "grok".to_owned(),
        };
        let report = DiscoveryReport {
            skills: vec![always, huge, vendor],
            skips: vec![],
        };
        let why = why(&report, None, Some("hello"), None);
        let reasons: Vec<_> = why
            .activation
            .iter()
            .map(|a| (a.name.as_str(), a.reason))
            .collect();
        assert!(reasons.contains(&("a-small", ActivationReason::Injected)));
        assert!(reasons.contains(&("b-huge", ActivationReason::BudgetOmitted)));
        assert!(reasons.contains(&("personal", ActivationReason::VendorEmptyTriggers)));
    }

    #[test]
    fn why_user_invocable_false_is_still_injected() {
        let mut skill = Skill::new("model-only", "d", "x".repeat(80));
        skill.user_invocable = false;
        let report = DiscoveryReport {
            skills: vec![skill],
            skips: vec![],
        };
        let why = why(&report, None, Some("hello"), None);
        assert_eq!(why.activation.len(), 1);
        assert_eq!(why.activation[0].reason, ActivationReason::Injected);
        assert_eq!(why.activation[0].name, "model-only");
    }

    #[test]
    fn why_json_includes_code_on_skip_and_activation() {
        let skill = Skill::new("foo", "d", "x".repeat(80));
        let skip = SkillSkip {
            path: PathBuf::from("/b/foo/SKILL.md"),
            name: Some("foo".to_owned()),
            kind: SkipKind::NameCollision,
            detail: "lost".to_owned(),
            winner_path: Some(PathBuf::from("/a/foo/SKILL.md")),
        };
        let report = DiscoveryReport {
            skills: vec![skill],
            skips: vec![skip],
        };
        let why = why(&report, None, Some("hello"), None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(v["skips"][0]["kind"], "name_collision", "{json}");
        assert_eq!(v["skips"][0]["code"], "name_collision", "{json}");
        assert_eq!(why.skips[0].code(), "name_collision");
        let reason = v["activation"][0]["reason"]
            .as_str()
            .expect("activation reason");
        assert_eq!(v["activation"][0]["code"].as_str(), Some(reason), "{json}");
        assert_eq!(why.activation[0].code(), why.activation[0].reason.as_str());
    }

    #[test]
    fn why_json_includes_invocation_flags() {
        let mut hidden = Skill::new("hidden-slash", "d", "body");
        hidden.user_invocable = false;
        hidden.disable_model_invocation = false;
        let mut slash = Skill::new("slash-ok", "d", "body");
        slash.user_invocable = true;
        slash.disable_model_invocation = true;
        let report = DiscoveryReport {
            skills: vec![hidden, slash],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["user_invocable"], false,
            "why JSON must carry user_invocable like list JSON/XML: {json}"
        );
        assert_eq!(
            v["loaded"][0]["disable_model_invocation"], false,
            "why JSON must carry disable_model_invocation like list JSON/XML: {json}"
        );
        assert_eq!(v["loaded"][1]["user_invocable"], true, "{json}");
        assert_eq!(v["loaded"][1]["disable_model_invocation"], true, "{json}");
        assert_eq!(
            v["loaded"][0]["description"], "d",
            "why JSON must carry description like list JSON/XML: {json}"
        );
        assert_eq!(v["loaded"][1]["description"], "d", "{json}");
        assert!(
            v["loaded"][0].get("userInvocable").is_none(),
            "why JSON flags must match list snake_case, not Skill camelCase: {json}"
        );
    }

    #[test]
    fn why_json_includes_argument_hint() {
        let mut hinted = Skill::new("slash-hint", "d", "body");
        hinted.argument_hint = Some("[name]".to_owned());
        let bare = Skill::new("no-hint", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["argument_hint"], "[name]",
            "why JSON must carry argument_hint like list JSON/XML: {json}"
        );
        assert!(
            v["loaded"][1]["argument_hint"].is_null(),
            "omitted argument_hint is null, not a camelCase key: {json}"
        );
        assert!(
            v["loaded"][0].get("argumentHint").is_none(),
            "why JSON argument_hint must match list snake_case, not Skill camelCase: {json}"
        );
    }

    #[test]
    fn why_json_includes_when_to_use() {
        let mut hinted = Skill::new("ranked", "d", "body");
        hinted.when_to_use = Some("after rebase".to_owned());
        let bare = Skill::new("no-when", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["when_to_use"], "after rebase",
            "why JSON must carry when_to_use like list JSON/XML: {json}"
        );
        assert!(
            v["loaded"][1]["when_to_use"].is_null(),
            "omitted when_to_use is null, not a camelCase key: {json}"
        );
        assert!(
            v["loaded"][0].get("whenToUse").is_none(),
            "why JSON when_to_use must match list snake_case, not Skill camelCase: {json}"
        );
    }

    #[test]
    fn why_json_includes_allowed_tools() {
        let mut hinted = Skill::new("tools-ok", "d", "body");
        hinted.allowed_tools = Some("Read Bash(git:*)".to_owned());
        let bare = Skill::new("no-tools", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["allowed_tools"], "Read Bash(git:*)",
            "why JSON must carry allowed_tools like list JSON/XML: {json}"
        );
        assert!(
            v["loaded"][1]["allowed_tools"].is_null(),
            "omitted allowed_tools is null, not a camelCase key: {json}"
        );
        assert!(
            v["loaded"][0].get("allowedTools").is_none(),
            "why JSON allowed_tools must match list snake_case, not Skill camelCase: {json}"
        );
    }

    #[test]
    fn why_json_includes_license() {
        let mut hinted = Skill::new("licensed", "d", "body");
        hinted.license = Some("MIT".to_owned());
        let bare = Skill::new("no-license", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["license"], "MIT",
            "why JSON must carry license like list JSON/XML: {json}"
        );
        assert!(
            v["loaded"][1]["license"].is_null(),
            "omitted license is null, not a camelCase key: {json}"
        );
    }

    #[test]
    fn why_json_includes_compatibility() {
        let mut hinted = Skill::new("gated", "d", "body");
        hinted.compatibility = Some("rust".to_owned());
        let bare = Skill::new("no-compat", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["compatibility"], "rust",
            "why JSON must carry compatibility like list JSON/XML: {json}"
        );
        assert!(
            v["loaded"][1]["compatibility"].is_null(),
            "omitted compatibility is null, not a camelCase key: {json}"
        );
    }

    #[test]
    fn why_json_includes_triggers() {
        let mut hinted = Skill::new("fired", "d", "body");
        hinted.triggers = vec!["git".to_owned(), "commit".to_owned()];
        let bare = Skill::new("no-triggers", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["triggers"],
            serde_json::json!(["git", "commit"]),
            "why JSON must carry triggers like list JSON/XML: {json}"
        );
        assert_eq!(
            v["loaded"][1]["triggers"],
            serde_json::json!([]),
            "empty triggers is [] on why JSON, not omitted: {json}"
        );
    }

    #[test]
    fn why_json_includes_metadata() {
        let mut hinted = Skill::new("annotated", "d", "body");
        hinted
            .metadata
            .insert("author".to_owned(), "A & B".to_owned());
        hinted
            .metadata
            .insert("version".to_owned(), "1.0".to_owned());
        let bare = Skill::new("no-metadata", "d", "body");
        let report = DiscoveryReport {
            skills: vec![hinted, bare],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(
            v["loaded"][0]["metadata"],
            serde_json::json!({"author": "A & B", "version": "1.0"}),
            "why JSON must carry metadata like list JSON/XML: {json}"
        );
        assert_eq!(
            v["loaded"][1]["metadata"],
            serde_json::json!({}),
            "empty metadata is {{}} on why JSON, not omitted: {json}"
        );
    }

    #[test]
    fn why_json_omitted_invocation_flags_keep_pre90_defaults() {
        // Cached why JSON from before PR 90 has no invocation flags.
        let json = r#"{
            "loaded":[{"name":"demo","source":"agents","path":"/tmp/demo/SKILL.md"}],
            "skips":[],
            "activation":[]
        }"#;
        let report: WhyReport = serde_json::from_str(json).expect("old why JSON");
        assert_eq!(report.loaded.len(), 1, "{json}");
        assert!(
            report.loaded[0].user_invocable,
            "omitted user_invocable must stay true (pre-90 default): {json}"
        );
        assert!(
            !report.loaded[0].disable_model_invocation,
            "omitted disable_model_invocation must stay false (pre-90 default): {json}"
        );
        assert!(
            report.loaded[0].description.is_empty(),
            "omitted description must stay empty: {json}"
        );
        assert!(
            report.loaded[0].argument_hint.is_none(),
            "omitted argument_hint must stay None: {json}"
        );
        assert!(
            report.loaded[0].when_to_use.is_none(),
            "omitted when_to_use must stay None: {json}"
        );
        assert!(
            report.loaded[0].allowed_tools.is_none(),
            "omitted allowed_tools must stay None: {json}"
        );
        assert!(
            report.loaded[0].triggers.is_empty(),
            "omitted triggers must stay empty: {json}"
        );
        assert!(
            report.loaded[0].metadata.is_empty(),
            "omitted metadata must stay empty: {json}"
        );
    }

    #[test]
    fn why_json_source_is_wire_name_like_list_xml() {
        let mut extra = Skill::new("demo", "d", "body");
        extra.source = SkillSource::ExtraPath;
        let mut vendor = Skill::new("home-note", "d", "body");
        vendor.source = SkillSource::Vendor {
            name: "claude".to_owned(),
        };
        let report = DiscoveryReport {
            skills: vec![extra, vendor],
            skips: vec![],
        };
        let why = why(&report, None, None, None);
        let json = serde_json::to_string(&why).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(v["loaded"][0]["source"], "extra", "{json}");
        assert_eq!(v["loaded"][1]["source"], "claude", "{json}");
        let old: WhyReport = serde_json::from_str(
            r#"{"loaded":[{"name":"demo","source":"extraPath"}],"skips":[],"activation":[]}"#,
        )
        .expect("old extraPath");
        assert_eq!(old.loaded[0].source, SkillSource::ExtraPath);
    }

    #[test]
    fn skill_summary_json_keys_have_list_xml_siblings() {
        use crate::activate::format_available_skills_xml;

        // Machine-readable list/why JSON and list XML must share these
        // keys. path is location in official skills-ref XML. Catalog and
        // load stay text envelopes (when_to_use / allowed_tools /
        // argument_hint / license / compatibility / triggers / metadata).
        const FIELDS: &[(&str, &str)] = &[
            ("name", "name"),
            ("description", "description"),
            ("source", "source"),
            ("path", "location"),
            ("user_invocable", "user_invocable"),
            ("disable_model_invocation", "disable_model_invocation"),
            ("argument_hint", "argument_hint"),
            ("when_to_use", "when_to_use"),
            ("triggers", "triggers"),
            ("allowed_tools", "allowed_tools"),
            ("license", "license"),
            ("compatibility", "compatibility"),
            ("metadata", "metadata"),
        ];

        let mut skill = Skill::new("slash-ok", "palette", "body");
        skill.source = SkillSource::ExtraPath;
        skill.source_path = Some(PathBuf::from("/tmp/slash-ok/SKILL.md"));
        skill.user_invocable = false;
        skill.disable_model_invocation = true;
        skill.argument_hint = Some("[name]".to_owned());
        skill.when_to_use = Some("after rebase".to_owned());
        skill.triggers = vec!["git".to_owned(), "A & B".to_owned()];
        skill.allowed_tools = Some("Read Bash".to_owned());
        skill.license = Some("MIT".to_owned());
        skill.compatibility = Some("rust".to_owned());
        skill
            .metadata
            .insert("author".to_owned(), "A & B".to_owned());
        skill
            .metadata
            .insert("version".to_owned(), "1.0".to_owned());

        let summary = SkillSummary::from(&skill);
        let json = serde_json::to_value(&summary).expect("summary serde");
        let xml = format_available_skills_xml(std::slice::from_ref(&skill));
        for (json_key, xml_tag) in FIELDS {
            assert!(
                json.get(*json_key).is_some(),
                "SkillSummary JSON must keep {json_key}: {json}"
            );
            assert!(
                json.get(*json_key)
                    .is_some_and(|v| !v.is_null() || *json_key == "path"),
                "populated SkillSummary.{json_key} must not be omitted: {json}"
            );
            let open = format!("<{xml_tag}>");
            assert!(
                xml.contains(&open),
                "list XML must emit <{xml_tag}> for SkillSummary.{json_key}: {xml}"
            );
        }
        assert_eq!(
            json["triggers"],
            serde_json::json!(["git", "A & B"]),
            "populated SkillSummary.triggers must copy from Skill (empty [] is a vacuous pass): {json}"
        );
        assert!(
            xml.contains("<triggers>git, A &amp; B</triggers>"),
            "list XML must emit populated triggers, not an empty tag: {xml}"
        );
        assert_eq!(
            json["metadata"],
            serde_json::json!({"author": "A & B", "version": "1.0"}),
            "populated SkillSummary.metadata must copy from Skill (empty {{}} is a vacuous pass): {json}"
        );
        assert!(
            xml.contains("<metadata>author=A &amp; B, version=1.0</metadata>"),
            "list XML must emit populated metadata, not an empty tag: {xml}"
        );
        assert!(
            json.get("argumentHint").is_none() && json.get("allowedTools").is_none(),
            "SkillSummary keys stay snake_case, not Skill camelCase: {json}"
        );
    }
}
