//! Skip-kind taxonomy. Frozen after this PR.

use std::fmt::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::skill::Skill;

/// Why a candidate `SKILL.md` was not loaded.
///
/// v1 set is frozen. Additions are a new versioned variant plus a corpus
/// fixture. Do not add `BudgetOmitted`, `Disabled`, `Ignored`,
/// `VendorDenylist`, or `InvocationOff` until they are real skip rows
/// with fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    /// File could not be read.
    Unreadable,
    /// Frontmatter/body failed agentskills parse/validation.
    ParseError,
    /// Frontmatter `name` does not match parent directory.
    NameDirectoryMismatch,
    /// Same name already loaded from a higher-priority path (`winner_path` set).
    NameCollision,
    /// Loose `SKILL.md` in a skills root instead of `dir/<name>/SKILL.md`.
    RootFile,
}

impl SkipKind {
    /// Frozen v1 set, in declaration order.
    pub const fn all() -> [Self; 5] {
        [
            Self::Unreadable,
            Self::ParseError,
            Self::NameDirectoryMismatch,
            Self::NameCollision,
            Self::RootFile,
        ]
    }

    /// Wire name (`snake_case`, Bline parity).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::ParseError => "parse_error",
            Self::NameDirectoryMismatch => "name_directory_mismatch",
            Self::NameCollision => "name_collision",
            Self::RootFile => "root_file",
        }
    }
}

impl fmt::Display for SkipKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A skill package that discovery found but did not load.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSkip {
    /// Path to the `SKILL.md` (or unreadable path).
    pub path: PathBuf,
    /// Frontmatter name when parse got far enough; otherwise `None`.
    #[serde(default)]
    pub name: Option<String>,
    pub kind: SkipKind,
    /// Human-readable reason (parse error text, etc.).
    pub detail: String,
    /// Winning path when `kind` is [`SkipKind::NameCollision`].
    ///
    /// Serialize stays `winnerPath`. Deserialize also accepts
    /// `winner_path` so a `SkillMiss` row is not silently winner-less.
    #[serde(default, alias = "winner_path")]
    pub winner_path: Option<PathBuf>,
}

impl Serialize for SkillSkip {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            path: &'a Path,
            #[serde(skip_serializing_if = "Option::is_none")]
            name: &'a Option<String>,
            kind: SkipKind,
            code: &'a str,
            detail: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            winner_path: &'a Option<PathBuf>,
        }
        Wire {
            path: &self.path,
            name: &self.name,
            kind: self.kind,
            code: self.kind.as_str(),
            detail: &self.detail,
            winner_path: &self.winner_path,
        }
        .serialize(serializer)
    }
}

impl SkillSkip {
    /// Stable machine code (`kind.as_str()`).
    pub fn code(&self) -> &'static str {
        self.kind.as_str()
    }

    /// Whether `load`/`why` should treat this skip as the requested name.
    ///
    /// A non-blank frontmatter `name` is an identity. The `SKILL.md` parent
    /// directory is also an identity for every skip except [`SkipKind::RootFile`]
    /// (that parent is the skills root, not a package). Blank or
    /// whitespace-only peeked names fall through to the package directory.
    pub fn matches_requested_name(&self, want: &str) -> bool {
        let want = want.trim();
        if want.is_empty() {
            return false;
        }
        if crate::parse::is_path_component_skill_name(want) {
            return false;
        }
        if self.name.as_deref().is_some_and(|n| {
            !crate::parse::is_path_component_skill_name(n)
                && crate::parse::skill_names_equal(n, want)
        }) {
            return true;
        }
        if matches!(self.kind, SkipKind::RootFile) {
            return false;
        }
        skill_md_package_name(&self.path).is_some_and(|n| crate::parse::skill_names_equal(n, want))
    }

    /// True when discover refused a host `--path` / `paths` or
    /// `--user-dir` / `user_dir` token (collapse or line separator).
    ///
    /// That skip is not a package identity. Named `load` / `why`
    /// still peel it so a host sees WHAT and which flag to change.
    pub(crate) fn is_host_token_refuse(&self) -> bool {
        self.kind == SkipKind::Unreadable
            && self.name.is_none()
            && (self.detail.contains("collapses after whitespace trim")
                || self.detail.contains("line separator"))
    }
}

/// Parent directory of a `SKILL.md` / `skill.md` path, when that is the file name.
pub(crate) fn skill_md_package_name(path: &Path) -> Option<&str> {
    let file = path.file_name().and_then(|n| n.to_str())?;
    if !file.eq_ignore_ascii_case("SKILL.md") {
        return None;
    }
    crate::parse::skill_md_package_dir_name(path)
}

/// Result of multi-root skill discovery, including skips for `why`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub skills: Vec<Skill>,
    pub skips: Vec<SkillSkip>,
}

/// TSV skip rows (`skip\tkind\tpath\tdetail`) for CLI list stderr,
/// CLI why stdout, and MCP catalog/xml text (stdio has no stderr).
///
/// Path and detail go through [`crate::sanitize_error_token`] so a
/// leftover SKILL.md skip cannot split the row (U+2028) or leak an
/// em dash. Extra-path refuse already sanitizes `skip.path` at
/// construction; leftover implicit paths do not.
pub fn format_skip_tsv(skips: &[SkillSkip]) -> String {
    let mut out = String::new();
    for skip in skips {
        let _ = writeln!(
            out,
            "skip\t{}\t{}\t{}",
            skip.kind.as_str(),
            crate::sanitize_error_token(&skip.path.display().to_string()),
            crate::sanitize_error_token(&skip.detail)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::{DiscoveryReport, SkillSkip, SkipKind};

    #[test]
    fn as_str_is_snake_case() {
        assert_eq!(SkipKind::Unreadable.as_str(), "unreadable");
        assert_eq!(SkipKind::ParseError.as_str(), "parse_error");
        assert_eq!(
            SkipKind::NameDirectoryMismatch.as_str(),
            "name_directory_mismatch"
        );
        assert_eq!(SkipKind::NameCollision.as_str(), "name_collision");
        assert_eq!(SkipKind::RootFile.as_str(), "root_file");
    }

    #[test]
    fn skill_skip_serializes_code_matching_kind() {
        for kind in SkipKind::all() {
            let skip = SkillSkip {
                path: PathBuf::from("/tmp/x/SKILL.md"),
                name: Some("x".to_owned()),
                kind,
                detail: "d".to_owned(),
                winner_path: None,
            };
            assert_eq!(skip.code(), kind.as_str());
            let json = serde_json::to_string(&skip).expect("ser");
            let v: serde_json::Value = serde_json::from_str(&json).expect("json");
            assert_eq!(v["code"].as_str(), Some(kind.as_str()), "json={json}");
            assert_eq!(v["kind"].as_str(), Some(kind.as_str()), "json={json}");
        }
    }

    #[test]
    fn serde_round_trip_and_wire_names() {
        for kind in SkipKind::all() {
            let json = serde_json::to_string(&kind).expect("ser");
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            let back: SkipKind = serde_json::from_str(&json).expect("de");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn taxonomy_is_exactly_five_v1_kinds() {
        let kinds = SkipKind::all();
        assert_eq!(kinds.len(), 5);
        // Exhaustive match is the freeze: a new variant fails to compile.
        for kind in kinds {
            match kind {
                SkipKind::Unreadable
                | SkipKind::ParseError
                | SkipKind::NameDirectoryMismatch
                | SkipKind::NameCollision
                | SkipKind::RootFile => {}
            }
        }
    }

    #[test]
    fn deferred_kinds_are_not_in_v1() {
        for wire in [
            "budget_omitted",
            "disabled",
            "ignored",
            "vendor_denylist",
            "invocation_off",
        ] {
            let json = format!("\"{wire}\"");
            assert!(
                serde_json::from_str::<SkipKind>(&json).is_err(),
                "v1 must reject {wire}"
            );
        }
    }

    #[test]
    fn serde_accepts_snake_winner_path() {
        // SkillMiss JSON uses `winner_path`. SkillSkip serialize uses
        // `winnerPath`. A host that feeds a miss row back into SkillSkip
        // must not stay winner-less (PR 213 leftover on skip wire).
        let json = r#"{"path":"/x/SKILL.md","kind":"name_collision","detail":"d","winner_path":"/a/foo/SKILL.md"}"#;
        let skip: SkillSkip = serde_json::from_str(json).expect("snake winner_path");
        assert_eq!(
            skip.winner_path.as_deref(),
            Some(std::path::Path::new("/a/foo/SKILL.md")),
            "snake winner_path must populate SkillSkip, not stay skipped None"
        );
        let out = serde_json::to_string(&skip).expect("ser");
        assert!(
            out.contains("winnerPath"),
            "SkillSkip serialize must still emit winnerPath: {out}"
        );
        assert!(
            !out.contains("winner_path"),
            "SkillSkip serialize must not switch to snake winner_path: {out}"
        );
    }

    /// SkillMiss `winner_path` must populate SkillSkip for any host string,
    /// not only `/a/foo/SKILL.md`. A path segment named `winner_path` would
    /// false-fail a `contains("winner_path")` serialize check.
    fn host_winner_path() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("/a/foo/SKILL.md".to_owned()),
            Just("/tmp/winner_path/source_path/SKILL.md".to_owned()),
            Just("/home/user/.agents/skills/foo/SKILL.md".to_owned()),
            Just("/tmp/ünicode dir/技能/SKILL.md".to_owned()),
            Just(r"C:\Users\foo\SKILL.md".to_owned()),
            ".{1,64}",
            prop::collection::vec("[A-Za-z0-9._-]{1,10}", 1usize..5)
                .prop_map(|parts| format!("/{}/SKILL.md", parts.join("/"))),
        ]
    }

    proptest! {
        #[test]
        fn serde_snake_winner_path_for_any_host_path(
            winner in host_winner_path(),
            skip_path in host_winner_path(),
        ) {
            let json = serde_json::json!({
                "path": skip_path,
                "kind": "name_collision",
                "detail": "d",
                "winner_path": winner,
            })
            .to_string();
            let skip: SkillSkip = serde_json::from_str(&json).expect("snake winner_path");
            prop_assert_eq!(
                skip.winner_path.as_deref(),
                Some(std::path::Path::new(&winner))
            );
            prop_assert_eq!(skip.path.as_path(), std::path::Path::new(&skip_path));
            let out = serde_json::to_value(&skip).expect("ser");
            prop_assert_eq!(
                out.get("winnerPath").and_then(|v| v.as_str()),
                Some(winner.as_str())
            );
            prop_assert!(
                out.get("winner_path").is_none(),
                "SkillSkip serialize must still emit winnerPath: {out}"
            );
        }
    }

    #[test]
    fn skill_skip_keeps_winner_path() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/b/foo/SKILL.md"),
            name: Some("foo".to_owned()),
            kind: SkipKind::NameCollision,
            detail: "already loaded".to_owned(),
            winner_path: Some(PathBuf::from("/tmp/a/foo/SKILL.md")),
        };
        let json = serde_json::to_string(&skip).expect("ser");
        assert!(json.contains("winnerPath"), "json={json}");
        assert!(json.contains("nameCollision") || json.contains("name_collision"));
        let back: SkillSkip = serde_json::from_str(&json).expect("de");
        assert_eq!(back, skip);
        assert_eq!(
            back.winner_path.as_deref(),
            Some(std::path::Path::new("/tmp/a/foo/SKILL.md"))
        );
    }

    #[test]
    fn host_token_refuse_is_not_a_package_identity() {
        let collapse = SkillSkip {
            path: PathBuf::from(" /.."),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "--path / paths token collapses after whitespace trim".to_owned(),
            winner_path: None,
        };
        assert!(collapse.is_host_token_refuse());
        assert!(
            !collapse.matches_requested_name("demo"),
            "collapse token is not the skill name demo"
        );
        let other = SkillSkip {
            path: PathBuf::from("/tmp/demo/SKILL.md"),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "Is a directory".to_owned(),
            winner_path: None,
        };
        assert!(!other.is_host_token_refuse());
    }

    #[test]
    fn matches_requested_name_uses_package_dir_when_nameless() {
        let parse = SkillSkip {
            path: PathBuf::from("/tmp/demo/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        assert!(parse.matches_requested_name("DEMO"));
        assert!(!parse.matches_requested_name("other"));
        let root = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills/SKILL.md"),
            name: None,
            kind: SkipKind::RootFile,
            detail: "put the file in a named subdirectory.".to_owned(),
            winner_path: None,
        };
        assert!(!root.matches_requested_name("skills"));
        assert!(!root.matches_requested_name("   "));
        let named_root = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills/SKILL.md"),
            name: Some("loose".to_owned()),
            kind: SkipKind::RootFile,
            detail: "put the file in a named subdirectory.".to_owned(),
            winner_path: None,
        };
        assert!(
            named_root.matches_requested_name("loose"),
            "frontmatter name remains an identity on root_file skips"
        );
        assert!(named_root.matches_requested_name("LOOSE"));
        assert!(!named_root.matches_requested_name("skills"));
    }

    #[test]
    fn matches_requested_name_nfkc_dot_components_in_path() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/wanted/evil/‥/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        assert!(
            skip.matches_requested_name("wanted"),
            "package dir must collapse NFKC `..` like extra-path, not treat ‥ as the name"
        );
        assert!(!skip.matches_requested_name("evil"));
        assert!(!skip.matches_requested_name(".."));
        assert!(!skip.matches_requested_name("‥"));
    }

    #[test]
    fn matches_requested_name_nfkc_dot_peek_is_not_identity() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/wanted/SKILL.md"),
            name: Some("．".to_owned()),
            kind: SkipKind::ParseError,
            detail: "name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        assert!(
            skip.matches_requested_name("wanted"),
            "package dir remains the identity when the peek is an NFKC path component"
        );
        assert!(
            !skip.matches_requested_name("."),
            "NFKC `.` is a path component, not a skill name"
        );
        assert!(!skip.matches_requested_name("．"));
    }

    #[test]
    fn matches_requested_name_blank_peeked_name_uses_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/skill.md"),
            name: Some("  ".to_owned()),
            kind: SkipKind::ParseError,
            detail: "name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        assert!(
            skip.matches_requested_name("demo"),
            "whitespace-only peek name is not an identity"
        );
        assert!(skip.matches_requested_name("DEMO"));
        assert!(!skip.matches_requested_name("   "));
    }

    #[test]
    fn matches_requested_name_trims_padded_peeked_name() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/other/SKILL.md"),
            name: Some(" foo ".to_owned()),
            kind: SkipKind::ParseError,
            detail: "name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        assert!(skip.matches_requested_name("foo"));
        assert!(
            skip.matches_requested_name("other"),
            "package dir remains an identity when the peeked name is different"
        );
    }

    #[test]
    fn matches_requested_name_named_and_nameless_keep_distinct_identities() {
        let named = SkillSkip {
            path: PathBuf::from("/tmp/other/SKILL.md"),
            name: Some("alpha".to_owned()),
            kind: SkipKind::ParseError,
            detail: "invalid YAML: name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        let nameless = SkillSkip {
            path: PathBuf::from("/tmp/alpha/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        assert!(named.matches_requested_name("alpha"));
        assert!(nameless.matches_requested_name("alpha"));
        assert!(
            named.matches_requested_name("other"),
            "named skip still matches its package dir so why/load do not call it unknown"
        );
        assert!(!nameless.matches_requested_name("other"));
    }

    #[test]
    fn matches_requested_name_parentdir_skill_md_uses_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/wanted/other/../SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        assert!(
            skip.matches_requested_name("wanted"),
            "wanted/other/../SKILL.md must be the wanted package, not other"
        );
        assert!(!skip.matches_requested_name("other"));
    }

    #[test]
    fn discovery_report_default_is_empty() {
        let report = DiscoveryReport::default();
        assert!(report.skills.is_empty());
        assert!(report.skips.is_empty());
        let json = serde_json::to_string(&report).expect("ser");
        let back: DiscoveryReport = serde_json::from_str(&json).expect("de");
        assert_eq!(back, report);
    }

    #[test]
    fn format_skip_tsv_leftover_hostile_detail_stays_one_row() {
        // Leftover analog of extra-path line-sep refuse (skip.path is
        // sanitized at construction). A leftover SKILL.md skip still
        // goes through format_skip_tsv raw. U+2028 / U+2014 in path or
        // detail must not split catalog/xml skip TSV (MCP appends the
        // same rows after the prompt fragment).
        assert_eq!(
            super::format_skip_tsv(&[]),
            "",
            "empty skips must not emit a skip row"
        );
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/evil\u{2028}root/SKILL.md"),
            name: Some("loose".to_owned()),
            kind: SkipKind::RootFile,
            detail: "put the file\u{2028}in a named\u{2014}subdirectory.".to_owned(),
            winner_path: None,
        };
        let tsv = super::format_skip_tsv(&[skip]);
        assert!(
            tsv.starts_with("skip\troot_file\t"),
            "leftover skip must stay a TSV row: {tsv:?}"
        );
        assert_eq!(
            tsv.chars().filter(|&c| c == '\n').count(),
            1,
            "leftover skip TSV must stay one row: {tsv:?}"
        );
        assert!(
            !tsv.contains('\u{2028}') && !tsv.contains('\u{2014}'),
            "U+2028 / em dash must not leak into skip TSV: {tsv:?}"
        );
        assert!(
            tsv.contains("evil?root") && tsv.contains("named-subdirectory"),
            "hostile leftover path and detail must be sanitized in place: {tsv:?}"
        );
    }
}
