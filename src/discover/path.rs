//! Path expansion, ignore prefixes, and walk-root identity.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

use super::host_token::{
    host_token_collapses_after_whitespace, nfkc_dot_path_components, path_has_line_separator,
    str_has_line_separator,
};

thread_local! {
    static HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Ancestors of `cwd` through the nearest `.git` (cwd first).
///
/// When no `.git` exists, the walk is `cwd` only so a nested tree
/// without a repo does not climb into an unrelated parent.
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
        out
    } else {
        out.truncate(1);
        out
    }
}

pub(super) fn home_dir() -> Option<PathBuf> {
    let override_home = HOME_OVERRIDE.with(|o| o.borrow().clone());
    if override_home.is_some() {
        return override_home;
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(super) fn expand_user_skills_dir(cwd: &Path, user_dir: Option<&Path>) -> Option<PathBuf> {
    // Same `~` / `~/` expand as extra-path and ignore. MCP and quoted
    // CLI `--user-dir` have no shell, unlike a typed `~/skills`.
    // Relative user_dir joins discover cwd, same as extra-path.
    // Empty or whitespace-only is not a directory (not cwd).
    let user_dir = user_dir?;
    if user_dir
        .to_str()
        .is_some_and(host_token_collapses_after_whitespace)
    {
        return None;
    }
    let expanded = match user_dir.to_str() {
        Some(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            expand_tilde(raw)
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
    Some(nfkc_dot_path_components(&expanded))
}

pub(super) fn expand_extra_path_arg(raw: &str, cwd: &Path) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = expand_tilde(raw);
    let expanded = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    Some(nfkc_dot_path_components(&expanded))
}

pub(super) fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
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
            let expanded = expand_tilde(raw);
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
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            HOME_OVERRIDE.with(|o| *o.borrow_mut() = self.0.take());
        }
    }
    let previous = HOME_OVERRIDE.with(|o| o.replace(home));
    let _restore = Restore(previous);
    f()
}
