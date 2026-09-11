//! Host-branchable load / why / validate miss.
//!
//! Presentation lives here so [`crate::discover`] does not format
//! "did you mean" or skip peels. This module does not import discover.

use std::path::PathBuf;

use serde::Serialize;

use crate::skip::SkillSkip;

/// Wire name when `load` / `why` matched no skill and no skip.
pub const UNKNOWN_SKILL_KIND: &str = "unknown_skill";

/// Host-branchable load / why miss. Display is the one-line text.
///
/// ```
/// let miss = craftbag::unknown_or_skipped_skill("no-such", &[]);
/// assert_eq!(miss.error_kind, craftbag::UNKNOWN_SKILL_KIND);
/// assert!(miss.path.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillMiss {
    /// Stable token: [`UNKNOWN_SKILL_KIND`] or a skip [`SkipKind::as_str`].
    #[serde(rename = "error_kind")]
    pub error_kind: &'static str,
    /// Same text as CLI stderr / MCP `content[0].text`.
    pub error: String,
    /// Skip or validate `SKILL.md` when known. Omitted on `unknown_skill`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Winning `SKILL.md` when `error_kind` is `name_collision`.
    /// Omitted on every other miss so hosts do not scrape `lost to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner_path: Option<PathBuf>,
}

impl SkillMiss {
    /// True when no matching skill or skip exists.
    pub fn is_not_found(&self) -> bool {
        self.error_kind == UNKNOWN_SKILL_KIND
    }
}

impl std::fmt::Display for SkillMiss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

/// Classify a `load` miss so hosts can branch without scraping Display.
///
/// A matching skip row (parse error, name/dir mismatch, unreadable
/// package) is not "unknown". Blank peeked names and the `SKILL.md`
/// parent directory (except root-file skips) are identities too.
/// Peeked `.` / `..` are path components, not skill names. The
/// message includes skip kind, package identity, and the SKILL.md
/// path so extra-path `.` / `..` (joined to discover cwd) stay
/// locatable. Name, path, and detail go through
/// [`crate::sanitize_error_token`] so the line cannot split.
///
/// Peeked skip names can hint a hyphen/underscore or edit-distance-1
/// typo. Pass loaded names through [`unknown_or_skipped_skill_named`].
pub fn unknown_or_skipped_skill(name: &str, skips: &[SkillSkip]) -> SkillMiss {
    unknown_or_skipped_skill_named(name, skips, std::iter::empty())
}

/// Same as [`unknown_or_skipped_skill`], plus loaded skill names for a
/// single-token typo hint (`did you mean review-pr?`).
///
/// Peeked skip names are included automatically. Two different close
/// tokens produce no hint. Path-like names keep the path hint only.
pub fn unknown_or_skipped_skill_named<'a>(
    name: &str,
    skips: &[SkillSkip],
    skill_names: impl IntoIterator<Item = &'a str>,
) -> SkillMiss {
    let want = name.trim();
    let skip = skips
        .iter()
        .find(|s| s.matches_requested_name(want))
        .or_else(|| skips.iter().find(|s| s.is_host_token_refuse()));
    match skip {
        Some(skip) if skip.is_host_token_refuse() => {
            let flag = skip
                .host_token
                .map(crate::skip::HostTokenField::flag_name)
                .unwrap_or("--path / paths");
            let shown = crate::sanitize_error_token(name);
            let detail = crate::sanitize_error_token(&skip.detail);
            SkillMiss {
                error_kind: skip.kind.as_str(),
                error: format!("unknown skill: {shown}; refused {flag}: {detail}"),
                path: Some(skip.path.clone()),
                winner_path: skip.winner_path.clone(),
            }
        }
        Some(skip) => SkillMiss {
            error_kind: skip.kind.as_str(),
            error: format!(
                "skipped skill: {} ({}) at {}: {}",
                crate::sanitize_error_token(skip_display_name(skip, want)),
                skip.kind.as_str(),
                crate::sanitize_error_token(&skip.path.display().to_string()),
                crate::sanitize_error_token(&skip.detail)
            ),
            path: Some(skip.path.clone()),
            winner_path: skip.winner_path.clone(),
        },
        None => {
            let shown = crate::sanitize_error_token(name);
            let error = if requested_name_looks_like_path(name) {
                format!(
                    "unknown skill: {shown} (looks like a path; pass the frontmatter name and --path / paths)"
                )
            } else {
                let mut names: Vec<&str> = skill_names.into_iter().collect();
                for skip in skips {
                    if let Some(n) = skip.name.as_deref() {
                        if !crate::parse::is_path_component_skill_name(n) {
                            names.push(n);
                        }
                    }
                }
                match close_skill_suggestion(name, names.iter().copied()) {
                    Some(hint) => {
                        let hint = crate::sanitize_error_token(hint);
                        format!("unknown skill: {shown} (did you mean {hint}?)")
                    }
                    None => format!("unknown skill: {shown}"),
                }
            };
            SkillMiss {
                error_kind: UNKNOWN_SKILL_KIND,
                error,
                path: None,
                winner_path: None,
            }
        }
    }
}

/// Fold used by [`crate::parse::skill_names_equal`]: NFKC, trim, case fold.
pub(super) fn fold_skill_token(name: &str) -> String {
    crate::parse::normalize_skill_name(name.trim()).to_lowercase()
}

/// One close catalog token, or none when zero or two+ distinct tokens match.
pub(super) fn close_skill_suggestion<'a>(
    want: &str,
    names: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let folded_want = fold_skill_token(want);
    if folded_want.is_empty() {
        return None;
    }
    let mut found: Option<(&'a str, String)> = None;
    for name in names {
        let folded = fold_skill_token(name);
        if folded.is_empty() || folded == folded_want {
            continue;
        }
        if !is_close_skill_token(&folded_want, &folded) {
            continue;
        }
        match &found {
            None => found = Some((name, folded)),
            Some((_, prev)) => {
                if prev != &folded {
                    return None;
                }
            }
        }
    }
    found.map(|(name, _)| name)
}

pub(super) fn is_close_skill_token(want: &str, cand: &str) -> bool {
    hyphen_underscore_equiv(want, cand) || edit_distance_is_one(want, cand)
}

pub(super) fn hyphen_underscore_equiv(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let fold = |s: &str| {
        s.chars()
            .map(|c| if c == '_' { '-' } else { c })
            .collect::<String>()
    };
    fold(a) == fold(b)
}

/// Unicode-scalar Levenshtein distance of exactly one.
pub(super) fn edit_distance_is_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (short, long) = if a.len() <= b.len() {
        (&a[..], &b[..])
    } else {
        (&b[..], &a[..])
    };
    let diff = long.len() - short.len();
    if diff > 1 {
        return false;
    }
    if diff == 0 {
        let mut mismatches = 0usize;
        for (x, y) in short.iter().zip(long.iter()) {
            if x != y {
                mismatches += 1;
                if mismatches > 1 {
                    return false;
                }
            }
        }
        return mismatches == 1;
    }
    let mut i = 0usize;
    let mut skipped = false;
    for ch in long {
        if i < short.len() && *ch == short[i] {
            i += 1;
        } else if !skipped {
            skipped = true;
        } else {
            return false;
        }
    }
    true
}

/// `load ./pkg` / `why ./pkg` is a package root, not a frontmatter name.
pub(super) fn requested_name_looks_like_path(name: &str) -> bool {
    let t = name.trim();
    t.contains('/') || t.contains('\\')
}

/// Error text when `load` cannot return a skill.
///
/// Same Display as [`unknown_or_skipped_skill`]. Prefer that when the
/// host can read [`SkillMiss::error_kind`].
pub fn unknown_or_skipped_skill_message(name: &str, skips: &[SkillSkip]) -> String {
    unknown_or_skipped_skill(name, skips).error
}

pub(super) fn skip_display_name<'a>(skip: &'a SkillSkip, want: &'a str) -> &'a str {
    skip.name
        .as_deref()
        .map(str::trim)
        .filter(|n| !crate::parse::is_path_component_skill_name(n))
        .or_else(|| crate::skip::skill_md_package_name(&skip.path))
        .unwrap_or(want)
}
