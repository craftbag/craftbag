//! Doctor: why a skill loaded, skipped, or did not auto-inject.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::activate::{ProgressiveBudgets, filter_skills, progressive_budgets, trigger_matches};
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
    pub source: SkillSource,
    pub path: Option<PathBuf>,
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
    /// is unknown.
    pub fn unknown_skill_message(&self) -> Option<String> {
        let want = self.query.as_deref()?;
        if self.loaded.is_empty() && self.skips.is_empty() {
            Some(format!("unknown skill: {want}"))
        } else {
            None
        }
    }
}

/// Explain loaded vs skipped skills and optional activation decisions.
///
/// Does not take [`crate::DiscoveryOptions`], so disabled-by-name and vendor
/// denylist are not activation reasons.
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
        .map(|s| SkillSummary {
            name: s.name.clone(),
            source: s.source.clone(),
            path: s.source_path.clone(),
        })
        .collect();
    let skips: Vec<SkillSkip> = report
        .skips
        .iter()
        .filter(|s| match q {
            None => true,
            Some(want) => s.matches_requested_name(want),
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
    if skill.triggers.is_empty() && is_vendor_compat(&skill.source) {
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

fn is_vendor_compat(source: &SkillSource) -> bool {
    match source {
        SkillSource::Vendor { name } => matches!(name.as_str(), "claude" | "cursor" | "grok"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivationReason, why};
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
        assert!(why.skips[0].winner_path.is_some());
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
}
