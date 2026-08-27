//! Public types, SKILL.md parse, discovery, and activation selector.
//!
//! [`DiscoveryOptions::default`] sets `implicit_roots: true` so
//! [`discover`] walks cwd-to-git `.agents` / vendor trees and
//! `$HOME/.agents` / vendor trees.
//! CLI `--no-implicit-roots` and MCP `implicit_roots: false` turn that
//! walk off; extra `paths` and `user_skills_dir` still load.
//!
//! [`SkillMiss`] peels `error_kind`, `error`, and `path` so a leftover-only
//! host can branch without scraping Display. `unknown_skill` omits `path`.
//! A `name_collision` skip also peels `winner_path`. Other misses omit it.
//!
//! List JSON, why JSON, and list XML share [`SkillSummary`]
//! (`description`, invocation flags, `argument_hint`, `when_to_use`,
//! `triggers`, `allowed_tools`, `license`, `compatibility`, `metadata`).
//! A new field on that type must land in all three wires
//! (`skill_summary_json_keys_have_list_xml_siblings`). Catalog stays cheap.
//! [`format_load_message`] is the text envelope (`License`,
//! `Compatibility`, `Metadata`, `Allowed tools`, and host extras
//! when set).
//!
//! [`validate_path_with_options`] accepts a SKILL.md file or package directory
//! (joins `SKILL.md` / `skill.md`). Success is [`ValidationReport`]
//! (no `error_kind`). A miss is [`ValidationReport::miss`]. CLI
//! `validate --json` and MCP `skills_validate` share that report.
//!
//! [`format_skip_tsv`] is the skip TSV source (`skip\tkind\tpath\tdetail`)
//! for CLI list stderr, CLI why stdout, and MCP catalog/xml text.
//! Do not inline those rows on a new text surface.

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
pub use skip::{DiscoveryReport, SkillSkip, SkipKind, format_skip_tsv};
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
    /// The walk is cwd-to-git `.agents` / `$HOME/.agents`, not the whole
    /// cwd-to-git tree and not `user_skills_dir`.
    #[test]
    fn crate_root_docs_name_implicit_roots_default() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        // Substring `implicit_roots: true` is also in
        // "does not set `implicit_roots: true`".
        assert!(
            docs.contains("[`DiscoveryOptions::default`] sets `implicit_roots: true`"),
            "crate-root rustdoc must say Default sets implicit_roots: true (not an inverted sentence): {docs}"
        );
        assert!(
            docs.contains("cwd-to-git `.agents`"),
            "crate-root rustdoc must attach .agents to cwd-to-git, not walk the whole tree: {docs}"
        );
        assert!(
            docs.contains("`$HOME/.agents`"),
            "crate-root rustdoc must name implicit HOME .agents, not user_skills_dir: {docs}"
        );
        // Substring `--no-implicit-roots` / `implicit_roots: false` /
        // `user_skills_dir` is also in "does not load `user_skills_dir`".
        assert!(
            docs.contains("CLI `--no-implicit-roots` and MCP `implicit_roots: false`"),
            "crate-root rustdoc must map CLI --no-implicit-roots and MCP implicit_roots: false (not an inverted sentence): {docs}"
        );
        assert!(
            docs.contains("`user_skills_dir` still load"),
            "crate-root rustdoc must say user_skills_dir still loads when implicit_roots is off (not an inverted sentence): {docs}"
        );
        assert!(
            super::DiscoveryOptions::default().implicit_roots,
            "documented default must stay true"
        );
    }

    /// A host adding a [`super::SkillSummary`] field should see the
    /// sibling lock on the crate root, not only in why.rs. List JSON,
    /// why JSON, and list XML share that type. Catalog stays cheap.
    /// Load is the text envelope.
    #[test]
    fn crate_root_docs_name_skill_summary_siblings() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("List JSON, why JSON, and list XML share [`SkillSummary`]"),
            "crate-root rustdoc must say list/why JSON + list XML share SkillSummary: {docs}"
        );
        assert!(
            docs.contains("skill_summary_json_keys_have_list_xml_siblings"),
            "crate-root rustdoc must name the sibling-lock test: {docs}"
        );
        assert!(
            docs.contains("Catalog stays cheap"),
            "crate-root rustdoc must say catalog stays cheap (not SkillSummary JSON): {docs}"
        );
        assert!(
            docs.contains("text envelope"),
            "crate-root rustdoc must say load is the text envelope: {docs}"
        );
    }

    /// A leftover-only host should see [`super::SkillMiss`] on the crate
    /// root, not only in discover.rs. Branch on `error_kind` and `path`.
    /// Do not scrape Display. `unknown_skill` omits `path`.
    /// `name_collision` also peels `winner_path`.
    #[test]
    fn crate_root_docs_name_skill_miss() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("[`SkillMiss`]"),
            "crate-root rustdoc must name SkillMiss so a leftover-only host does not scrape Display: {docs}"
        );
        assert!(
            docs.contains("error_kind"),
            "crate-root rustdoc must name SkillMiss.error_kind: {docs}"
        );
        assert!(
            docs.contains("leftover-only"),
            "crate-root rustdoc must address leftover-only hosts: {docs}"
        );
        assert!(
            docs.contains("scraping Display"),
            "crate-root rustdoc must say branch without scraping Display: {docs}"
        );
        assert!(
            docs.contains("unknown_skill") && docs.contains("omits `path`"),
            "crate-root rustdoc must say unknown_skill omits path (do not invent one): {docs}"
        );
        assert!(
            docs.contains("winner_path") && docs.contains("name_collision"),
            "crate-root rustdoc must name SkillMiss.winner_path on name_collision: {docs}"
        );
        let unknown = super::unknown_or_skipped_skill("no-such-skill", &[]);
        assert_eq!(unknown.error_kind, super::UNKNOWN_SKILL_KIND);
        assert!(
            unknown.path.is_none(),
            "documented unknown_skill miss must omit path"
        );
    }

    /// A leftover-only host should see [`super::ValidationReport`] on the
    /// crate root, not only in discover.rs. CLI `validate --json` and MCP
    /// `skills_validate` share that success shape (no `error_kind`).
    #[test]
    fn crate_root_docs_name_validation_report() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("[`ValidationReport`]"),
            "crate-root rustdoc must name ValidationReport like MCP skills_validate: {docs}"
        );
        assert!(
            docs.contains("package directory") && docs.contains("SKILL.md"),
            "crate-root rustdoc must name package dir like CLI validate --help: {docs}"
        );
        assert!(
            docs.contains("no `error_kind`") || docs.contains("no error_kind"),
            "crate-root rustdoc must say success has no error_kind: {docs}"
        );
        assert!(
            docs.contains("validate --json") && docs.contains("skills_validate"),
            "crate-root rustdoc must map CLI validate --json and MCP skills_validate: {docs}"
        );
    }

    /// After PR 278-280, [`super::format_skip_tsv`] is the skip TSV
    /// source. A new text surface must not inline `skip\tkind\tpath\tdetail`.
    #[test]
    fn crate_root_docs_name_format_skip_tsv() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("[`format_skip_tsv`]"),
            "crate-root rustdoc must name format_skip_tsv so a new surface does not inline skip TSV: {docs}"
        );
        assert!(
            docs.contains("CLI why") && docs.contains("list") && docs.contains("catalog"),
            "crate-root rustdoc must name CLI why / list / catalog as format_skip_tsv callers: {docs}"
        );
        let collapsed = docs.replace('\n', " ");
        assert!(
            !collapsed.contains("why inlines") && !collapsed.contains("still inlined"),
            "crate-root rustdoc must not say why inlines skip TSV: {docs}"
        );
    }
}
