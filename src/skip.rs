//! Skip-kind taxonomy. Frozen after this PR.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::skill::Skill;

/// Why a candidate `SKILL.md` was not loaded.
///
/// v1 set is frozen. Additions are a new versioned variant plus a corpus
/// fixture. Do not add `BudgetOmitted`, `Disabled`, `VendorDenylist`, or
/// `InvocationOff` until they are real skip rows with fixtures.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSkip {
    /// Path to the `SKILL.md` (or unreadable path).
    pub path: PathBuf,
    /// Frontmatter name when parse got far enough; otherwise `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: SkipKind,
    /// Human-readable reason (parse error text, etc.).
    pub detail: String,
    /// Winning path when `kind` is [`SkipKind::NameCollision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner_path: Option<PathBuf>,
}

impl SkillSkip {
    /// Whether `load`/`why` should treat this skip as the requested name.
    ///
    /// Frontmatter `name` wins when present. Nameless unreadable and parse
    /// skips still match the `SKILL.md` parent directory so they are not
    /// reported as unknown.
    pub fn matches_requested_name(&self, want: &str) -> bool {
        let want = want.trim();
        if want.is_empty() {
            return false;
        }
        if let Some(n) = self.name.as_deref() {
            return n.eq_ignore_ascii_case(want);
        }
        matches!(self.kind, SkipKind::Unreadable | SkipKind::ParseError)
            && skill_md_package_name(&self.path).is_some_and(|n| n.eq_ignore_ascii_case(want))
    }
}

/// Parent directory of a `SKILL.md` / `skill.md` path, when that is the file name.
pub(crate) fn skill_md_package_name(path: &Path) -> Option<&str> {
    let file = path.file_name().and_then(|n| n.to_str())?;
    if !file.eq_ignore_ascii_case("SKILL.md") {
        return None;
    }
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
}

/// Result of multi-root skill discovery, including skips for `why`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub skills: Vec<Skill>,
    pub skips: Vec<SkillSkip>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
}
