//! Public types, SKILL.md parse, discovery, and activation selector.

mod activate;
mod discover;
mod error;
mod parse;
mod skill;
mod skip;
mod source;
mod why;

pub use activate::{
    DEFAULT_ACTIVATE_HINT, FormatOptions, ListFormat, ProgressiveBudgets, filter_skills,
    format_available_skills_xml, format_body_header, format_catalog, format_load_message,
    format_package_envelope, parse_list_format, progressive_budgets, rank_skills_for_catalog,
    skill_relevance_score, trigger_matches, truncate_skill_body_for_budget, unknown_list_format,
};
pub use discover::{
    CURSOR_VENDOR_DENYLIST, DiscoveryOptions, ValidationReport, discover, find_skill_by_name,
    unknown_or_skipped_skill_message, validate_path, validate_path_with_options,
    walk_cwd_to_git_root, watch_dirs, with_home_override,
};
pub use error::{Error, ParseError};
pub use parse::{
    normalize_skill_name, parse_skill, skill_name_is_ascii_policy, skill_name_matches_directory,
    skill_names_equal, validate_skill_name,
};
pub use skill::{
    SKILL_BODY_LINE_SOFT_WARN, SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS,
    SKILL_MD_MAX_BYTES, SKILL_NAME_MAX_CHARS, Skill,
};
pub use skip::{DiscoveryReport, SkillSkip, SkipKind};
pub use source::SkillSource;
pub use why::{ActivationDecision, ActivationReason, SkillSummary, WhyReport, why};

/// Package version from `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_nonzero() {
        assert!(!super::version().is_empty());
    }
}
