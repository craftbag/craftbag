//! Host-token hygiene: line separators, whitespace collapse, NFKC dots.

use std::path::{Component, Path, PathBuf};

use crate::skip::SkillSkip;

pub(super) fn one_line_error(raw: impl std::fmt::Display) -> String {
    crate::sanitize_error_token(&raw.to_string())
}

/// Rewrite NFKC-equivalent `.` / `..` components to ASCII so join and
/// `Path::is_dir` treat them as cwd / parent, not as extra-path names.
pub(super) fn nfkc_dot_path_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => {
                if let Some(text) = s.to_str() {
                    let n = crate::parse::normalize_skill_name(text);
                    let n = n.trim();
                    if n == "." {
                        out.push(".");
                        continue;
                    }
                    if n == ".." {
                        out.push("..");
                        continue;
                    }
                }
                out.push(s);
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when NFKC / trim rewrite turns an absolute token into `/`
/// (`/ ..`, `/\t..`). Explicit `/` and `/..` stay host-chosen roots.
/// Relative `foo/ ‥` still rewrites to parent, not this refuse.
pub(super) fn nfkc_absolute_rewrites_to_fs_root(raw: &str) -> bool {
    if raw == "/" || raw == "/.." {
        return false;
    }
    let rewritten = nfkc_dot_path_components(Path::new(raw));
    let mut comps = rewritten.components();
    match (comps.next(), comps.next(), comps.next()) {
        (Some(Component::RootDir), None, _) => true,
        (Some(Component::RootDir), Some(Component::ParentDir), None) => true,
        (Some(Component::RootDir), Some(Component::Normal(s)), None) if s == ".." => true,
        _ => false,
    }
}

/// True when a host token contains a line separator (U+000A, U+000D,
/// U+2028, U+2029). Used before Path join so Windows cannot drop the
/// control character as a component.
pub(super) fn str_has_line_separator(s: &str) -> bool {
    s.chars()
        .any(|ch| matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

/// True when trim turns a relative-looking token into `/` / `/..`, or
/// a Path component has leading/trailing whitespace so `evil /..`
/// lexical-collapses to cwd the same way `evil\n/..` did.
pub(super) fn host_token_collapses_after_whitespace(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Whole-token padded `.` / `..` (including NFKC) still mean cwd.
    // Relative `foo/ ‥` is still an extra-path / ignore rewrite.
    // Absolute `/ ..` nfkc-rewrites to `/` and must collapse.
    let n = crate::parse::normalize_skill_name(trimmed);
    let n = n.trim();
    if n == "." || n == ".." {
        return false;
    }
    if trimmed != raw && Path::new(trimmed).is_absolute() {
        return true;
    }
    if Path::new(raw).components().any(|c| match c {
        Component::Normal(s) => s.to_str().is_some_and(|t| {
            let inner = t.trim();
            if inner.is_empty() {
                return true;
            }
            let n = crate::parse::normalize_skill_name(inner);
            let n = n.trim();
            if n == "." || n == ".." {
                return false;
            }
            t != inner
        }),
        _ => false,
    }) {
        return true;
    }
    nfkc_absolute_rewrites_to_fs_root(raw)
}

/// True when a path component contains a line separator (U+000A, U+000D,
/// U+2028, U+2029). Hosts that split `watch_dirs` on newline would see
/// a fake root. Discover refuses the extra-path or user_dir instead of
/// loading it.
pub(super) fn path_has_line_separator(p: &Path) -> bool {
    p.components().any(|c| {
        let Some(s) = c.as_os_str().to_str() else {
            return false;
        };
        str_has_line_separator(s)
    })
}

/// Which host field produced a refused token. Named so skip details
/// tell a first-time CLI or MCP host which flag/field to change.
#[derive(Clone, Copy)]
pub(super) enum HostPathField {
    ExtraPath,
    UserDir,
    Validate,
}

impl HostPathField {
    pub(super) fn collapse_detail(self) -> &'static str {
        match self {
            Self::ExtraPath => "--path / paths token collapses after whitespace trim",
            Self::UserDir => "--user-dir / user_dir token collapses after whitespace trim",
            Self::Validate => {
                "validate / skills_validate path token collapses after whitespace trim"
            }
        }
    }

    pub(super) fn line_sep_detail(self) -> &'static str {
        match self {
            Self::ExtraPath => "--path / paths component contains a line separator",
            Self::UserDir => "--user-dir / user_dir component contains a line separator",
            Self::Validate => "validate / skills_validate path component contains a line separator",
        }
    }

    fn missing_path_detail(self, shown: &str) -> String {
        match self {
            Self::ExtraPath => format!(
                "path does not exist: {shown} (pass --path / paths as a SKILL.md file or a package directory that contains SKILL.md)"
            ),
            Self::UserDir => format!(
                "path does not exist: {shown} (pass --user-dir / user_dir as a directory of skill packages)"
            ),
            Self::Validate => format!(
                "path does not exist: {shown} (pass a SKILL.md file or a package directory that contains SKILL.md)"
            ),
        }
    }

    fn not_a_directory_detail(self, shown: &str) -> String {
        match self {
            Self::ExtraPath => format!(
                "--path / paths is not a directory: {shown} (pass a SKILL.md file or a package directory that contains SKILL.md)"
            ),
            Self::UserDir => {
                format!("--user-dir / user_dir is not a directory: {shown}")
            }
            Self::Validate => format!("path is not a directory: {shown}"),
        }
    }

    fn unreadable_path_detail(self, shown: &str) -> String {
        match self {
            Self::ExtraPath => format!("--path / paths is unreadable: {shown}"),
            Self::UserDir => format!("--user-dir / user_dir is unreadable: {shown}"),
            Self::Validate => format!("path is unreadable: {shown}"),
        }
    }

    fn token_field(self) -> crate::skip::HostTokenField {
        match self {
            Self::ExtraPath => crate::skip::HostTokenField::ExtraPath,
            Self::UserDir => crate::skip::HostTokenField::UserDir,
            Self::Validate => crate::skip::HostTokenField::Validate,
        }
    }
}

pub(super) fn skip_line_separator_root(
    root: &Path,
    field: HostPathField,
    skips: &mut Vec<SkillSkip>,
) {
    // Sanitize path for list/why JSON and miss lines so hosts never
    // echo a raw line separator from skip.path.
    let skill_md = root.join("SKILL.md");
    skips.push(SkillSkip::host_token_refuse(
        PathBuf::from(crate::sanitize_error_token(&skill_md.display().to_string())),
        field.line_sep_detail().to_owned(),
        field.token_field(),
    ));
}

pub(super) fn skip_whitespace_collapse_token(
    raw: &str,
    field: HostPathField,
    skips: &mut Vec<SkillSkip>,
) {
    skips.push(SkillSkip::host_token_refuse(
        PathBuf::from(crate::sanitize_error_token(raw)),
        field.collapse_detail().to_owned(),
        field.token_field(),
    ));
}

/// Host-asked extra-path or user_dir that is not a loadable SKILL.md
/// file and is not a directory. Implicit missing `.agents` stays
/// silent in [`load_skills_from_dir`]; only this door reports it.
pub(super) fn skip_unresolvable_host_path(
    path: &Path,
    field: HostPathField,
    skips: &mut Vec<SkillSkip>,
) {
    let shown = crate::sanitize_error_token(&path.display().to_string());
    // Stat first (never open). FIFO / socket / device would hang on
    // open; `metadata` is enough to tell missing vs not-a-directory.
    let detail = match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => return,
        Ok(_) => field.not_a_directory_detail(&shown),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => field.missing_path_detail(&shown),
        Err(_) => field.unreadable_path_detail(&shown),
    };
    let detail = one_line_error(detail);
    skips.push(SkillSkip::host_token_refuse(
        PathBuf::from(shown),
        detail,
        field.token_field(),
    ));
}

/// True when a Normal component is `.` / `..` only after trim
/// (` ..`, `..\n`). NFKC `‥` stays an extra-path rewrite.
pub(super) fn component_is_whitespace_padded_dot(s: &str) -> bool {
    let n = crate::parse::normalize_skill_name(s);
    let trimmed = n.trim();
    if trimmed != "." && trimmed != ".." {
        return false;
    }
    if s == "." || s == ".." {
        return false;
    }
    n != "." && n != ".."
}
