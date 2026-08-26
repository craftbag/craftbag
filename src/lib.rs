//! Public types, SKILL.md parse, discovery, and activation selector.
//!
//! [`DiscoveryOptions::default`] sets `implicit_roots: true` so
//! [`discover`] walks cwd-to-git and `$HOME` `.agents` / vendor trees.
//! CLI `--no-implicit-roots` and MCP `implicit_roots: false` turn that
//! walk off; extra `paths` and `user_skills_dir` still load.

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
    CURSOR_VENDOR_DENYLIST, DiscoveryOptions, SkillMiss, UNKNOWN_SKILL_KIND, ValidationReport,
    discover, find_skill_by_name, unknown_or_skipped_skill, unknown_or_skipped_skill_message,
    validate_path, validate_path_with_options, walk_cwd_to_git_root, watch_dirs,
    with_home_override,
};
pub use error::{Error, ParseError, sanitize_error_token};
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

    /// A host adding CLI `--no-implicit-roots` / MCP `implicit_roots`
    /// should see the default on the crate root, not only in discover.rs.
    #[test]
    fn crate_root_docs_name_implicit_roots_default() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("implicit_roots: true"),
            "crate-root rustdoc must name DiscoveryOptions::default implicit_roots: true: {docs}"
        );
        assert!(
            docs.contains("--no-implicit-roots"),
            "crate-root rustdoc must map CLI --no-implicit-roots to implicit_roots: {docs}"
        );
        assert!(
            super::DiscoveryOptions::default().implicit_roots,
            "documented default must stay true"
        );
    }
}
