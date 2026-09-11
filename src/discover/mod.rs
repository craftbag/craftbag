//! Multi-root SKILL.md discovery. First name wins.

mod extra_path;
mod host_token;
mod load;
mod path;
mod walk;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Cursor vendor-shipped skill names never injected from `.cursor` roots.
/// Silent in v1 (no skip row).
pub const CURSOR_VENDOR_DENYLIST: &[&str] = &["shell", "canvas", "statusline"];

/// Options for multi-root skill discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOptions {
    /// Extra paths (`~` expanded). Relative paths join the discover `cwd`.
    /// Empty or whitespace-only items are ignored (not cwd).
    pub paths: Vec<String>,
    /// Path prefixes to ignore (`~` expanded). Relative prefixes join `cwd`.
    /// Empty or whitespace-only items are ignored (not cwd).
    /// A prefix whose component contains a line separator is dropped
    /// (same refuse as extra-path / user_dir). Lexical `evil\n/..`
    /// must not collapse to cwd and hide the walk. A token that
    /// collapses after whitespace trim (` /..`, `/ ..`, `evil /..`) is
    /// dropped the same way.
    pub ignore: Vec<String>,
    /// Skill names never returned (still skipped at load, no skip row).
    /// Same NFKC + case-fold identity as [`find_skill_by_name`] / `why`.
    pub disabled: Vec<String>,
    /// Host names: `bline`, `claude`, `cursor`, `grok`.
    pub vendor_roots: Vec<String>,
    /// Host-supplied user skills dir (`~` / `~/` expanded, relative
    /// paths join the discover `cwd`, same as `paths`). Empty or
    /// whitespace-only is ignored. A token that collapses after
    /// whitespace trim (` /..`, `/ ..`) is an unreadable skip (same
    /// refuse as extra-path).
    pub user_skills_dir: Option<PathBuf>,
    /// When true, names outside `a-z0-9-` are a `parse_error` skip.
    /// Default is off: Unicode / NFKC names still load.
    pub ascii_names: bool,
    /// Walk cwd-to-git `.agents` / vendor trees and `$HOME/.agents` /
    /// vendor trees. Default is true. When false,
    /// extra `paths` and optional `user_skills_dir` still load
    /// (collection-only). Empty `paths` plus no user dir returns an
    /// empty report, not an error.
    pub implicit_roots: bool,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            ignore: Vec::new(),
            disabled: Vec::new(),
            vendor_roots: Vec::new(),
            user_skills_dir: None,
            ascii_names: false,
            implicit_roots: true,
        }
    }
}

pub use path::{walk_cwd_to_git_root, with_home_override};
pub use walk::{
    ValidationReport, discover, find_skill_by_name, format_watch_dirs, validate_path,
    validate_path_with_options, watch_dirs,
};

#[cfg(test)]
use crate::miss::{
    unknown_or_skipped_skill, unknown_or_skipped_skill_message, unknown_or_skipped_skill_named,
};
#[cfg(test)]
use extra_path::{
    ExtraPathMd, classify_extra_path_md, extra_path_is_loose_collection,
    load_classified_extra_path_package,
};
#[cfg(test)]
use host_token::{
    host_token_collapses_after_whitespace, path_has_line_separator, str_has_line_separator,
};
#[cfg(test)]
use load::take_read_skill_md_paths;
#[cfg(test)]
use path::home_dir;
