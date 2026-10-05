//! Path expansion, ignore prefixes, and walk-root identity.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

use crate::source::SkillSource;

use super::host_token::{
    host_token_collapses_after_whitespace, nfkc_dot_path_components, path_has_line_separator,
    str_has_line_separator,
};

#[derive(Clone)]
enum HomeLock {
    /// Read `HOME`, then `USERPROFILE`.
    Env,
    /// `Some` is that directory, including a blank path.
    /// `None` is unset, even when the process has `HOME`.
    Forced(Option<PathBuf>),
}

thread_local! {
    static HOME_OVERRIDE: RefCell<HomeLock> = const { RefCell::new(HomeLock::Env) };
}

/// Ancestors of `cwd` through the nearest `.git` (cwd first).
///
/// When no `.git` exists, the walk is `cwd` only so a nested tree
/// without a repo does not climb into an unrelated parent. The
/// exception is a cwd already inside `.agents` or `.{vendor}`: keep
/// that directory and its parent, which is the project that holds the
/// skill tree.
pub fn walk_cwd_to_git_root(cwd: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current = Some(cwd.to_path_buf());
    let mut found_git = false;
    while let Some(dir) = current {
        out.push(dir.clone());
        if dir.join(".git").exists() {
            found_git = true;
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    if found_git {
        return out;
    }
    match out.iter().position(|dir| enclosing_skill_root(dir)) {
        Some(idx) => {
            let keep = idx.saturating_add(2);
            if keep < out.len() {
                out.truncate(keep);
            }
            out
        }
        None => {
            out.truncate(1);
            out
        }
    }
}

fn enclosing_skill_root(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name == ".agents" {
        return true;
    }
    SkillSource::VENDOR_TOKENS
        .iter()
        .any(|token| name.strip_prefix('.').is_some_and(|rest| rest == *token))
}

pub(super) fn home_dir() -> Option<PathBuf> {
    match HOME_OVERRIDE.with(|o| o.borrow().clone()) {
        HomeLock::Forced(home) => home,
        HomeLock::Env => std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from),
    }
}

/// `~` and `~/...` need a non-blank home. An empty override, `HOME=""`,
/// or whitespace-only home is the same as unset: callers must not join
/// the token onto cwd.
pub(super) fn nonempty_home() -> Option<PathBuf> {
    let home = home_dir()?;
    let blank = match home.to_str() {
        Some(text) => text.trim().is_empty(),
        None => home.as_os_str().is_empty(),
    };
    if blank { None } else { Some(home) }
}

/// Result of expanding one host path token.
#[derive(Debug)]
pub(super) enum ArgExpand {
    /// Absolute, or relative and safe to join onto the discover cwd.
    Ready(PathBuf),
    /// Token is `~` or `~/...` and home is unset or blank.
    /// Callers must not join it onto cwd.
    HomeUnset,
    /// No token, or empty / whitespace-only. Not cwd.
    Empty,
}

pub(super) fn expand_user_skills_dir(cwd: &Path, user_dir: Option<&Path>) -> ArgExpand {
    // Same `~` / `~/` expand as extra-path and ignore. MCP and quoted
    // CLI `--user-dir` have no shell, unlike a typed `~/skills`.
    // Relative user_dir joins discover cwd, same as extra-path.
    // Empty or whitespace-only is not a directory (not cwd).
    // `~` / `~/` with unset or blank home is [`ArgExpand::HomeUnset`],
    // not a cwd join onto a lookalike directory.
    let Some(user_dir) = user_dir else {
        return ArgExpand::Empty;
    };
    if user_dir
        .to_str()
        .is_some_and(host_token_collapses_after_whitespace)
    {
        return ArgExpand::Empty;
    }
    let expanded = match user_dir.to_str() {
        Some(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return ArgExpand::Empty;
            }
            match expand_tilde(raw) {
                TildeExpand::HomeUnset => return ArgExpand::HomeUnset,
                TildeExpand::Ready(path) => path,
            }
        }
        None => user_dir.to_path_buf(),
    };
    let expanded = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    // Same NFKC `.` / `..` rewrite as extra-path and ignore, so
    // `wanted/evil/‥` is the `wanted` user root, not a missing dir.
    ArgExpand::Ready(nfkc_dot_path_components(&expanded))
}

pub(super) fn expand_extra_path_arg(raw: &str, cwd: &Path) -> ArgExpand {
    let raw = raw.trim();
    if raw.is_empty() {
        return ArgExpand::Empty;
    }
    let expanded = match expand_tilde(raw) {
        TildeExpand::HomeUnset => return ArgExpand::HomeUnset,
        TildeExpand::Ready(path) => path,
    };
    let expanded = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    ArgExpand::Ready(nfkc_dot_path_components(&expanded))
}

enum TildeExpand {
    Ready(PathBuf),
    HomeUnset,
}

fn expand_tilde(raw: &str) -> TildeExpand {
    if raw == "~" {
        return match nonempty_home() {
            Some(home) => TildeExpand::Ready(home),
            None => TildeExpand::HomeUnset,
        };
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return match nonempty_home() {
            Some(home) => TildeExpand::Ready(home.join(rest)),
            None => TildeExpand::HomeUnset,
        };
    }
    TildeExpand::Ready(PathBuf::from(raw))
}

/// True when this ignore token is `~` or `~/...` and home is unset or blank.
/// Line-separator and trim-collapse tokens stay on their existing drop path.
pub(super) fn tilde_without_home(raw: &str) -> bool {
    if str_has_line_separator(raw) || host_token_collapses_after_whitespace(raw) {
        return false;
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return false;
    }
    matches!(expand_tilde(raw), TildeExpand::HomeUnset)
}

pub(super) struct IgnorePrefix {
    lexical: PathBuf,
    canonical: Option<PathBuf>,
}

pub(super) fn expand_ignore_list(cwd: &Path, paths: &[String]) -> Vec<IgnorePrefix> {
    paths
        .iter()
        .filter_map(|p| {
            // Host token first, before trim. `\n/..`.trim() is `/..`,
            // which Windows treats as a root-relative prefix. Windows
            // Path::components can also drop a control-char component,
            // so `evil\n/..` would collapse to cwd after lexical
            // normalize if we only inspected Path.
            if str_has_line_separator(p) || host_token_collapses_after_whitespace(p) {
                return None;
            }
            // Empty or whitespace-only is not a prefix (not discover cwd).
            let raw = p.trim();
            if raw.is_empty() {
                return None;
            }
            let expanded = match expand_tilde(raw) {
                TildeExpand::HomeUnset => return None,
                TildeExpand::Ready(path) => path,
            };
            let joined = if expanded.is_absolute() {
                expanded
            } else {
                cwd.join(expanded)
            };
            // Same NFKC `.` / `..` rewrite as extra-path arguments, then
            // lexical collapse so `wanted/evil/‥` is the `wanted` prefix.
            let joined = nfkc_dot_path_components(&joined);
            // Same refuse as extra-path / user_dir: a line-separator
            // component must not become a prefix (`evil\n/..` collapses
            // to cwd and would hide the walk).
            if path_has_line_separator(&joined) {
                return None;
            }
            let lexical = lexical_normalize(&joined);
            let canonical = joined
                .canonicalize()
                .ok()
                .or_else(|| lexical.canonicalize().ok());
            Some(IgnorePrefix { lexical, canonical })
        })
        .collect()
}

pub(super) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(super) fn path_is_ignored(path: &Path, ignore: &[IgnorePrefix]) -> bool {
    if ignore.is_empty() {
        return false;
    }
    ignore
        .iter()
        .any(|prefix| path_has_ignore_prefix(path, prefix))
}

pub(super) fn path_has_ignore_prefix(path: &Path, prefix: &IgnorePrefix) -> bool {
    let path_lex = lexical_normalize(path);
    if path_lex.starts_with(&prefix.lexical) || path.starts_with(&prefix.lexical) {
        return true;
    }
    let Some(prefix_canon) = prefix.canonical.as_deref() else {
        return false;
    };
    if path.starts_with(prefix_canon) || path_lex.starts_with(prefix_canon) {
        return true;
    }
    match path.canonicalize() {
        Ok(path_canon) => path_canon.starts_with(prefix_canon),
        Err(_) => false,
    }
}

pub(super) fn stays_under(path: &Path, ancestor: &Path) -> bool {
    let Ok(anc) = ancestor.canonicalize() else {
        return false;
    };
    let Ok(p) = path.canonicalize() else {
        return false;
    };
    p.starts_with(anc)
}

/// True when `a` and `b` are the same walk root (cwd vs `$HOME` symlink).
pub(super) fn same_walk_root(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ac), Ok(bc)) => ac == bc,
        _ => lexical_normalize(a) == lexical_normalize(b),
    }
}

/// True when `$HOME` is already in the cwd-to-git walk, so implicit
/// home `.agents` / vendor trees must not be loaded or watched again.
pub(super) fn implicit_home_already_walked(cwd_walk: &[PathBuf], home: &Path) -> bool {
    cwd_walk.iter().any(|dir| same_walk_root(dir, home))
}

/// Override the home directory for in-process tests. Not a host API.
///
/// Restores the previous override on return and on panic. Nested calls
/// restore the outer value, not always `None`.
#[doc(hidden)]
pub fn with_home_override<T>(home: Option<PathBuf>, f: impl FnOnce() -> T) -> T {
    let locked = match home {
        Some(path) => HomeLock::Forced(Some(path)),
        None => HomeLock::Env,
    };
    with_home_lock(locked, f)
}

/// In-process tests: [`home_dir`] stays unset even when the process has `HOME`.
#[cfg(test)]
pub(super) fn with_home_unset<T>(f: impl FnOnce() -> T) -> T {
    with_home_lock(HomeLock::Forced(None), f)
}

fn with_home_lock<T>(locked: HomeLock, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<HomeLock>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                HOME_OVERRIDE.with(|o| *o.borrow_mut() = previous);
            }
        }
    }
    let previous = HOME_OVERRIDE.with(|o| o.replace(locked));
    let _restore = Restore(Some(previous));
    f()
}
