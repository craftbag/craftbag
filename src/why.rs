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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationDecision {
    pub name: String,
    pub reason: ActivationReason,
    pub detail: String,
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
    let q = query.map(str::trim).filter(|s| !s.is_empty());
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
            Some(want) => s
                .name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(want)),
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
        query: q.map(ToOwned::to_owned),
    }
}

fn name_matches(query: Option<&str>, name: &str) -> bool {
    match query {
        None => true,
        Some(want) => name.eq_ignore_ascii_case(want),
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
}
