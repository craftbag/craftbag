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
//! (`skill_summary_json_keys_have_list_xml_siblings`). Catalog stays cheap
//! and omits `disable_model_invocation` (official client-guide). JSON, XML,
//! and TSV still list those rows.
//! [`format_load_message`] is the text envelope (`License`,
//! `Compatibility`, `Metadata`, `Allowed tools`, and host extras
//! when set). [`format_load_view`] can print an outline or one
//! heading section of that same SKILL.md body instead of the
//! whole body. It does not dump `scripts/` or `references/`.
//!
//! [`validate_path_with_options`] accepts a SKILL.md file or package directory
//! (joins `SKILL.md` / `skill.md`). Success is [`ValidationReport`]
//! (no `error_kind`). A miss is [`ValidationReport::miss`]. CLI
//! `validate --json` and MCP `skills_validate` share that report.
//!
//! [`format_skip_tsv`] is the skip TSV source (`skip\tkind\tpath\tdetail`)
//! for CLI list stderr, CLI why stdout, and MCP catalog/xml text.
//! [`format_list_tsv`] is default list TSV. [`format_why_text`] is CLI
//! why text and MCP `skills_why` format=text (loaded rows, skip TSV,
//! activation). [`format_watch_dirs`] is CLI `list --watch-dirs` and
//! MCP `skills_list format=watch`. Do not inline those rows on a new
//! text surface.

mod activate;
mod discover;
mod error;
mod miss;
mod parse;
mod sections;
mod skill;
mod skip;
mod source;
mod why;

pub use activate::{
    DEFAULT_ACTIVATE_HINT, FormatOptions, ListFormat, LoadView, ProgressiveBudgets, filter_skills,
    format_available_skills_xml, format_catalog, format_load_message, format_load_view,
    format_package_envelope, parse_list_format, progressive_budgets, rank_skills_for_catalog,
    skill_relevance_score, trigger_matches, unknown_list_format,
};
pub use discover::{
    CURSOR_VENDOR_DENYLIST, DiscoveryOptions, ValidationReport, discover, find_skill_by_name,
    format_watch_dirs, validate_path, validate_path_with_options, walk_cwd_to_git_root, watch_dirs,
    with_home_override,
};
pub use error::{Error, ParseError, sanitize_error_token};
pub use miss::{
    SkillMiss, UNKNOWN_SKILL_KIND, unknown_or_skipped_skill, unknown_or_skipped_skill_message,
    unknown_or_skipped_skill_named,
};
pub use parse::{
    normalize_skill_name, parse_skill, skill_name_is_ascii_policy, skill_name_matches_directory,
    skill_names_equal, validate_skill_name,
};
pub use sections::{
    CHARS_PER_TOKEN, SkillOutline, SkillSection, SkillSectionMeta, WHOLE_BODY_CHEAPER_TOKENS,
    estimate_tokens, outline_of, skill_section, split_sections, unknown_section_message,
};
pub use skill::{
    SKILL_BODY_LINE_SOFT_WARN, SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS,
    SKILL_MD_MAX_BYTES, SKILL_NAME_MAX_CHARS, Skill,
};
pub use skip::{
    DiscoveryReport, HostTokenField, SkillSkip, SkipKind, format_list_tsv, format_skip_tsv,
};
pub use source::SkillSource;
pub use why::{
    ActivationDecision, ActivationReason, SkillSummary, WhyReport, format_why_text, why,
};

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

    /// The walk is cwd-to-git `.agents` / `$HOME/.agents`, not the whole
    /// cwd-to-git tree. That inverted sentence drifted once (PRs 170-172).
    #[test]
    fn crate_root_docs_attach_agents_to_cwd_to_git() {
        let docs: String = include_str!("lib.rs")
            .lines()
            .filter(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            docs.contains("cwd-to-git `.agents`"),
            "crate-root rustdoc must attach .agents to cwd-to-git, not walk the whole tree: {docs}"
        );
        assert!(
            super::DiscoveryOptions::default().implicit_roots,
            "documented default must stay true"
        );
    }

    #[test]
    fn unknown_skill_miss_omits_path() {
        let unknown = super::unknown_or_skipped_skill("no-such-skill", &[]);
        assert_eq!(unknown.error_kind, super::UNKNOWN_SKILL_KIND);
        assert!(unknown.path.is_none(), "unknown_skill miss must omit path");
        let json = serde_json::to_value(&unknown).expect("miss serde");
        assert!(
            json.get("path").is_none(),
            "unknown_skill JSON omits path: {json}"
        );
        assert!(
            json.get("winner_path").is_none(),
            "unknown_skill JSON omits winner_path: {json}"
        );
    }

    #[test]
    fn name_collision_miss_peels_winner_path() {
        use std::path::PathBuf;

        use super::{SkillSkip, SkipKind, unknown_or_skipped_skill};

        let skip = SkillSkip {
            path: PathBuf::from("/tmp/b/foo/SKILL.md"),
            name: Some("foo".to_owned()),
            kind: SkipKind::NameCollision,
            detail: "lost to /tmp/a/foo/SKILL.md".to_owned(),
            winner_path: Some(PathBuf::from("/tmp/a/foo/SKILL.md")),
            ..SkillSkip::default()
        };
        let miss = unknown_or_skipped_skill("foo", std::slice::from_ref(&skip));
        assert_eq!(miss.error_kind, "name_collision");
        assert_eq!(miss.path.as_deref(), Some(skip.path.as_path()));
        assert_eq!(
            miss.winner_path.as_deref(),
            Some(std::path::Path::new("/tmp/a/foo/SKILL.md"))
        );
        let json = serde_json::to_value(&miss).expect("miss serde");
        assert_eq!(json["error_kind"], "name_collision");
        assert_eq!(json["winner_path"], "/tmp/a/foo/SKILL.md");
    }

    #[test]
    fn validation_report_success_has_no_error_kind() {
        let dir = tempfile::tempdir().expect("pkg");
        let pkg = dir.path().join("demo");
        std::fs::create_dir_all(&pkg).expect("dir");
        std::fs::write(
            pkg.join("SKILL.md"),
            "---\nname: demo\ndescription: ok\n---\nbody\n",
        )
        .expect("skill");
        let report = super::validate_path(&pkg);
        assert!(report.ok, "package directory must validate: {report:?}");
        assert!(report.miss().is_none(), "ok report has no miss");
        let json = serde_json::to_value(&report).expect("report serde");
        assert!(
            json.get("error_kind").is_none(),
            "success ValidationReport must not grow error_kind: {json}"
        );
    }
}
