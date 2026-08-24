//! Multi-root SKILL.md discovery. First name wins.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::parse::{parse_skill, peek_frontmatter_name, skill_name_matches_directory};
use crate::skill::Skill;
use crate::skip::{DiscoveryReport, SkillSkip, SkipKind};
use crate::source::SkillSource;

/// Cursor vendor-shipped skill names never injected from `.cursor` roots.
/// Silent in v1 (no skip row).
pub const CURSOR_VENDOR_DENYLIST: &[&str] = &["shell", "canvas", "statusline"];

thread_local! {
    static HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Options for multi-root skill discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryOptions {
    /// Extra paths (`~` expanded).
    pub paths: Vec<String>,
    /// Path prefixes to ignore (`~` expanded). Relative prefixes join `cwd`.
    pub ignore: Vec<String>,
    /// Skill names never returned (still skipped at load, no skip row).
    pub disabled: Vec<String>,
    /// Host names: `bline`, `claude`, `cursor`, `grok`.
    pub vendor_roots: Vec<String>,
    /// Host-supplied user skills dir.
    pub user_skills_dir: Option<PathBuf>,
}

/// Discover skills for `cwd` using the host-neutral root matrix.
///
/// Missing directories are not an error. Parse and IO problems become
/// [`SkillSkip`] rows.
pub fn discover(cwd: &Path, opts: &DiscoveryOptions) -> Result<DiscoveryReport, Error> {
    Ok(discover_report(cwd, opts))
}

/// Case-insensitive skill lookup by frontmatter `name`.
pub fn find_skill_by_name<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    skills.iter().find(|s| s.name.to_lowercase() == want)
}

/// Error text when `load` cannot return a skill.
///
/// A matching skip row (parse error, name/dir mismatch, unreadable
/// package) is not "unknown". Rows with no frontmatter name still match
/// the `SKILL.md` parent directory for unreadable and parse skips.
pub fn unknown_or_skipped_skill_message(name: &str, skips: &[SkillSkip]) -> String {
    let want = name.trim();
    let skip = skips.iter().find(|s| s.matches_requested_name(want));
    match skip {
        Some(skip) => format!(
            "skipped skill: {} ({}): {}",
            skip.name
                .as_deref()
                .or_else(|| crate::skip::skill_md_package_name(&skip.path))
                .unwrap_or(want),
            skip.kind.as_str(),
            skip.detail
        ),
        None => format!("unknown skill: {name}"),
    }
}

/// Result of validating one SKILL.md path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub path: PathBuf,
    pub ok: bool,
    pub name: Option<String>,
    pub errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<SkillSkip>,
}

/// Validate a SKILL.md path: readable, parse, and name/dir match.
pub fn validate_path(path: &Path) -> ValidationReport {
    let path_buf = path.to_path_buf();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ValidationReport {
                path: path_buf.clone(),
                ok: false,
                name: None,
                errors: vec![e.to_string()],
                skip: Some(SkillSkip {
                    path: path_buf,
                    name: None,
                    kind: SkipKind::Unreadable,
                    detail: e.to_string(),
                    winner_path: None,
                }),
            };
        }
    };
    match parse_skill(&content) {
        Ok(skill) => {
            if skill_name_matches_directory(path, &skill.name) {
                ValidationReport {
                    path: path_buf,
                    ok: true,
                    name: Some(skill.name),
                    errors: Vec::new(),
                    skip: None,
                }
            } else {
                let detail = format!(
                    "frontmatter name `{}` must match parent directory name",
                    skill.name
                );
                ValidationReport {
                    path: path_buf.clone(),
                    ok: false,
                    name: Some(skill.name.clone()),
                    errors: vec![detail.clone()],
                    skip: Some(SkillSkip {
                        path: path_buf,
                        name: Some(skill.name),
                        kind: SkipKind::NameDirectoryMismatch,
                        detail,
                        winner_path: None,
                    }),
                }
            }
        }
        Err(e) => {
            let name = peek_frontmatter_name(&content);
            ValidationReport {
                path: path_buf.clone(),
                ok: false,
                name: name.clone(),
                errors: vec![e.to_string()],
                skip: Some(SkillSkip {
                    path: path_buf,
                    name,
                    kind: SkipKind::ParseError,
                    detail: e.to_string(),
                    winner_path: None,
                }),
            }
        }
    }
}

fn discover_report(cwd: &Path, opts: &DiscoveryOptions) -> DiscoveryReport {
    let ignore = expand_ignore_list(cwd, &opts.ignore);
    let disabled = opts.disabled.clone();
    let mut skills = Vec::new();
    let mut skips = Vec::new();

    for dir in walk_cwd_to_git_root(cwd) {
        let agents = dir.join(".agents").join("skills");
        if !skip_if_dir_escapes(&agents, &dir, &mut skips) {
            load_skills_from_dir(
                &agents,
                SkillSource::Agents,
                &ignore,
                &disabled,
                &[],
                &mut skills,
                &mut skips,
            );
        }
        load_vendor_tree(&dir, opts, &ignore, &disabled, &mut skills, &mut skips);
    }

    if let Some(user_dir) = &opts.user_skills_dir {
        load_skills_from_dir(
            user_dir,
            SkillSource::User,
            &ignore,
            &disabled,
            &[],
            &mut skills,
            &mut skips,
        );
    }

    if let Some(home) = home_dir() {
        let agents = home.join(".agents").join("skills");
        if !skip_if_dir_escapes(&agents, &home, &mut skips) {
            load_skills_from_dir(
                &agents,
                SkillSource::Agents,
                &ignore,
                &disabled,
                &[],
                &mut skills,
                &mut skips,
            );
        }
        load_vendor_tree(&home, opts, &ignore, &disabled, &mut skills, &mut skips);
    }

    for raw in &opts.paths {
        load_extra_path(raw, &ignore, &disabled, &mut skills, &mut skips);
    }

    DiscoveryReport { skills, skips }
}

fn load_vendor_tree(
    root: &Path,
    opts: &DiscoveryOptions,
    ignore: &[PathBuf],
    disabled: &[String],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    for name in ["bline", "claude", "cursor", "grok"] {
        if !vendor_enabled(opts, name) {
            continue;
        }
        let dir = root.join(format!(".{name}")).join("skills");
        if skip_if_dir_escapes(&dir, root, skips) {
            continue;
        }
        let denylist: &[&str] = if name == "cursor" {
            CURSOR_VENDOR_DENYLIST
        } else {
            &[]
        };
        load_skills_from_dir(
            &dir,
            SkillSource::Vendor {
                name: name.to_owned(),
            },
            ignore,
            disabled,
            denylist,
            skills,
            skips,
        );
    }
}

fn vendor_enabled(opts: &DiscoveryOptions, name: &str) -> bool {
    opts.vendor_roots.iter().any(|v| v == name)
}

fn load_extra_path(
    raw: &str,
    ignore: &[PathBuf],
    disabled: &[String],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    let expanded = expand_tilde(raw);
    if expanded.is_file()
        && expanded
            .file_name()
            .is_some_and(|n| n == "SKILL.md" || n == "skill.md")
    {
        try_load_skill_file(
            &expanded,
            SkillSource::ExtraPath,
            ignore,
            disabled,
            &[],
            skills,
            skips,
        );
        return;
    }
    if !expanded.is_dir() {
        return;
    }
    let package_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| expanded.join(name))
        .find(|p| p.is_file());
    if let Some(skill_file) = package_md {
        try_load_skill_file(
            &skill_file,
            SkillSource::ExtraPath,
            ignore,
            disabled,
            &[],
            skills,
            skips,
        );
        return;
    }
    let skills_subdir = expanded.join("skills");
    let scan = if skills_subdir.is_dir() {
        if skip_if_dir_escapes(&skills_subdir, &expanded, skips) {
            return;
        }
        skills_subdir
    } else {
        expanded
    };
    load_skills_from_dir(
        &scan,
        SkillSource::ExtraPath,
        ignore,
        disabled,
        &[],
        skills,
        skips,
    );
}

fn walk_cwd_to_git_root(cwd: &Path) -> Vec<PathBuf> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let root = find_git_root(&cwd);
    let mut out = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        out.push(dir.clone());
        if let Some(ref r) = root {
            if &dir == r {
                break;
            }
        } else {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    out
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    let override_home = HOME_OVERRIDE.with(|o| o.borrow().clone());
    if override_home.is_some() {
        return override_home;
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn expand_tilde(raw: &str) -> PathBuf {
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

fn expand_ignore_list(cwd: &Path, paths: &[String]) -> Vec<PathBuf> {
    let cwd_abs = cwd
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(cwd));
    paths
        .iter()
        .map(|p| {
            let expanded = expand_tilde(p);
            let joined = if expanded.is_absolute() {
                expanded
            } else {
                cwd_abs.join(expanded)
            };
            lexical_normalize(&joined)
        })
        .collect()
}

fn lexical_normalize(path: &Path) -> PathBuf {
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

fn path_is_ignored(path: &Path, ignore: &[PathBuf]) -> bool {
    if ignore.is_empty() {
        return false;
    }
    ignore
        .iter()
        .any(|prefix| path_has_ignore_prefix(path, prefix))
}

fn path_has_ignore_prefix(path: &Path, prefix: &Path) -> bool {
    let path_lex = lexical_normalize(path);
    let prefix_lex = lexical_normalize(prefix);
    if path_lex.starts_with(&prefix_lex) || path.starts_with(prefix) {
        return true;
    }
    let prefix_canon = match prefix.canonicalize().or_else(|_| prefix_lex.canonicalize()) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if path.starts_with(&prefix_canon) || path_lex.starts_with(&prefix_canon) {
        return true;
    }
    match path.canonicalize() {
        Ok(path_canon) => path_canon.starts_with(&prefix_canon),
        Err(_) => false,
    }
}

fn stays_under(path: &Path, ancestor: &Path) -> bool {
    let Ok(anc) = ancestor.canonicalize() else {
        return false;
    };
    let Ok(p) = path.canonicalize() else {
        return false;
    };
    p.starts_with(anc)
}

fn skip_if_dir_escapes(dir: &Path, confine: &Path, skips: &mut Vec<SkillSkip>) -> bool {
    if dir.exists() && !stays_under(dir, confine) {
        skips.push(SkillSkip {
            path: dir.to_path_buf(),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "skills directory symlink escapes walk root".to_owned(),
            winner_path: None,
        });
        return true;
    }
    false
}

fn load_skills_from_dir(
    dir: &Path,
    source: SkillSource,
    ignore: &[PathBuf],
    disabled: &[String],
    denylist: &[&str],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            skips.push(SkillSkip {
                path: dir.to_path_buf(),
                name: None,
                kind: SkipKind::Unreadable,
                detail: e.to_string(),
                winner_path: None,
            });
            return;
        }
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == "SKILL.md" || n == "skill.md")
                && !path_is_ignored(&path, ignore)
            {
                skips.push(SkillSkip {
                    path,
                    name: None,
                    kind: SkipKind::RootFile,
                    detail: "put the file in a named subdirectory.".to_owned(),
                    winner_path: None,
                });
            }
            continue;
        }

        let skill_file = ["SKILL.md", "skill.md"]
            .into_iter()
            .map(|name| path.join(name))
            .find(|p| p.is_file());
        let Some(skill_file) = skill_file else {
            continue;
        };
        if !stays_under(&path, dir) {
            skips.push(SkillSkip {
                path: skill_file,
                name: None,
                kind: SkipKind::Unreadable,
                detail: "skill package symlink escapes walk root".to_owned(),
                winner_path: None,
            });
            continue;
        }
        try_load_skill_file(
            &skill_file,
            source.clone(),
            ignore,
            disabled,
            denylist,
            skills,
            skips,
        );
    }
}

fn try_load_skill_file(
    skill_file: &Path,
    source: SkillSource,
    ignore: &[PathBuf],
    disabled: &[String],
    denylist: &[&str],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    if path_is_ignored(skill_file, ignore) {
        return;
    }

    if !skill_md_stays_in_package(skill_file) {
        skips.push(SkillSkip {
            path: skill_file.to_path_buf(),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "SKILL.md symlink escapes package root".to_owned(),
            winner_path: None,
        });
        return;
    }

    let content = match std::fs::read_to_string(skill_file) {
        Ok(c) => c,
        Err(e) => {
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name: None,
                kind: SkipKind::Unreadable,
                detail: e.to_string(),
                winner_path: None,
            });
            return;
        }
    };

    match parse_skill(&content) {
        Ok(mut skill) => {
            if disabled.iter().any(|d| d == &skill.name) {
                return;
            }
            if denylist.iter().any(|d| *d == skill.name) {
                return;
            }
            if !skill_name_matches_directory(skill_file, &skill.name) {
                skips.push(SkillSkip {
                    path: skill_file.to_path_buf(),
                    name: Some(skill.name.clone()),
                    kind: SkipKind::NameDirectoryMismatch,
                    detail: format!(
                        "frontmatter name `{}` must match parent directory name",
                        skill.name
                    ),
                    winner_path: None,
                });
                return;
            }
            if let Some(winner) = skills.iter().find(|s| s.name == skill.name) {
                let winner_path = winner
                    .source_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("<already-loaded>"));
                skips.push(SkillSkip {
                    path: skill_file.to_path_buf(),
                    name: Some(skill.name.clone()),
                    kind: SkipKind::NameCollision,
                    detail: format!("lost to {}", winner_path.display()),
                    winner_path: Some(winner_path),
                });
                return;
            }
            skill.source = source;
            skill.source_path = Some(skill_file.to_path_buf());
            skills.push(skill);
        }
        Err(e) => {
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name: peek_frontmatter_name(&content),
                kind: SkipKind::ParseError,
                detail: e.to_string(),
                winner_path: None,
            });
        }
    }
}

fn skill_md_stays_in_package(skill_file: &Path) -> bool {
    let Some(pkg) = skill_file.parent() else {
        return false;
    };
    let Ok(pkg_canon) = pkg.canonicalize() else {
        return false;
    };
    let Ok(file_canon) = skill_file.canonicalize() else {
        return false;
    };
    file_canon.starts_with(&pkg_canon)
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

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_VENDOR_DENYLIST, DiscoveryOptions, discover, find_skill_by_name,
        unknown_or_skipped_skill_message, with_home_override,
    };
    use crate::skip::{SkillSkip, SkipKind};
    use crate::source::SkillSource;
    use std::fs;
    use std::path::PathBuf;

    fn empty_home_discover(
        cwd: &std::path::Path,
        opts: &DiscoveryOptions,
    ) -> crate::skip::DiscoveryReport {
        let home = tempfile::tempdir().expect("home");
        with_home_override(Some(home.path().to_path_buf()), || {
            discover(cwd, opts).expect("discover")
        })
    }

    fn write_skill(dir: &std::path::Path, name: &str, body: &str) {
        fs::create_dir_all(dir).expect("mkdir");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} skill\n---\n{body}\n"),
        )
        .expect("write");
    }

    #[test]
    fn cursor_vendor_denylist_is_frozen() {
        assert_eq!(CURSOR_VENDOR_DENYLIST, &["shell", "canvas", "statusline"]);
    }

    #[test]
    fn load_miss_names_parse_skip_not_unknown() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/Bad_Name/SKILL.md"),
            name: Some("Bad_Name".to_owned()),
            kind: SkipKind::ParseError,
            detail: "invalid YAML: name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        let msg = unknown_or_skipped_skill_message("bad_name", &[skip]);
        assert!(msg.contains("skipped skill: Bad_Name"), "msg={msg}");
        assert!(msg.contains("parse_error"), "msg={msg}");
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        assert_eq!(
            unknown_or_skipped_skill_message("no-such", &[]),
            "unknown skill: no-such"
        );
    }

    #[test]
    fn load_miss_names_unreadable_package_skip_not_unknown() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/SKILL.md"),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "Permission denied (os error 13)".to_owned(),
            winner_path: None,
        };
        let msg = unknown_or_skipped_skill_message("demo", &[skip]);
        assert!(
            msg.contains("skipped skill: demo"),
            "unreadable package must not look missing: {msg}"
        );
        assert!(msg.contains("unreadable"), "msg={msg}");
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }

    #[test]
    fn load_miss_parse_skip_without_frontmatter_name_uses_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/skill.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        let msg = unknown_or_skipped_skill_message("DEMO", &[skip]);
        assert!(
            msg.contains("skipped skill: demo"),
            "package dir is the identity when peek name is missing: {msg}"
        );
        assert!(msg.contains("parse_error"), "msg={msg}");
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }

    #[test]
    fn load_miss_root_file_and_empty_name_stay_unknown() {
        let root = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills/SKILL.md"),
            name: None,
            kind: SkipKind::RootFile,
            detail: "put the file in a named subdirectory.".to_owned(),
            winner_path: None,
        };
        let dir = SkillSkip {
            path: PathBuf::from("/tmp/.agents/skills"),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "Permission denied (os error 13)".to_owned(),
            winner_path: None,
        };
        assert_eq!(
            unknown_or_skipped_skill_message("skills", std::slice::from_ref(&root)),
            "unknown skill: skills"
        );
        assert_eq!(
            unknown_or_skipped_skill_message("skills", &[dir]),
            "unknown skill: skills"
        );
        assert_eq!(
            unknown_or_skipped_skill_message("   ", &[root]),
            "unknown skill:    "
        );
    }

    #[test]
    fn agents_root_loads_named_package() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".agents").join("skills").join("demo"),
            "demo",
            "hi",
        );
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "demo");
        assert_eq!(report.skills[0].source, SkillSource::Agents);
        assert!(report.skips.is_empty());
    }

    #[test]
    fn vendor_bline_is_off_by_default() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".bline").join("skills").join("legacy"),
            "legacy",
            "x",
        );
        let off = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(off.skills.is_empty());

        let on = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                vendor_roots: vec!["bline".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(on.skills.len(), 1);
        assert_eq!(
            on.skills[0].source,
            SkillSource::Vendor {
                name: "bline".to_owned()
            }
        );
    }

    #[test]
    fn first_name_wins_and_records_winner_path() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".agents").join("skills").join("foo"),
            "foo",
            "first",
        );
        let extra = tempfile::tempdir().expect("extra");
        write_skill(&extra.path().join("foo"), "foo", "second");
        let report = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                paths: vec![extra.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].content.trim(), "first");
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::NameCollision);
        assert!(report.skips[0].winner_path.is_some());
    }

    #[test]
    fn loose_skill_md_in_agents_root_is_root_file() {
        let root = tempfile::tempdir().expect("tmp");
        let skills = root.path().join(".agents").join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        fs::write(
            skills.join("SKILL.md"),
            "---\nname: loose\ndescription: loose\n---\nbody\n",
        )
        .expect("write");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(report.skills.is_empty());
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::RootFile);
    }

    #[test]
    fn cursor_denylist_is_silent() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".cursor").join("skills").join("shell"),
            "shell",
            "denied",
        );
        write_skill(
            &root.path().join(".cursor").join("skills").join("ok-one"),
            "ok-one",
            "keep",
        );
        let report = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                vendor_roots: vec!["cursor".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "ok-one");
        assert!(report.skips.is_empty());
    }

    #[test]
    fn disabled_name_is_silent() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".agents").join("skills").join("off"),
            "off",
            "x",
        );
        let report = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                disabled: vec!["off".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(report.skills.is_empty());
        assert!(report.skips.is_empty());
    }

    #[test]
    fn user_skills_dir_is_user_source() {
        let cwd = tempfile::tempdir().expect("cwd");
        let user = tempfile::tempdir().expect("user");
        write_skill(&user.path().join("mine"), "mine", "u");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                user_skills_dir: Some(user.path().to_path_buf()),
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].source, SkillSource::User);
    }

    #[test]
    fn find_skill_by_name_is_case_insensitive() {
        let mut skill = crate::skill::Skill::new("Demo", "d", "");
        skill.name = "Demo".to_owned();
        let skills = vec![skill];
        assert!(find_skill_by_name(&skills, "demo").is_some());
        assert!(find_skill_by_name(&skills, "").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_skills_dir_is_skip_not_silent() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("tmp");
        let skills = root.path().join(".agents").join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        write_skill(&skills.join("hidden"), "hidden", "x");
        let original = fs::metadata(&skills).expect("meta").permissions();
        struct Restore<'a>(&'a std::path::Path, fs::Permissions);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let _ = fs::set_permissions(self.0, self.1.clone());
            }
        }
        let _restore = Restore(&skills, original.clone());
        let mut locked = original.clone();
        locked.set_mode(0o000);
        fs::set_permissions(&skills, locked).expect("chmod");
        if fs::read_dir(&skills).is_ok() {
            return;
        }
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(
            report.skips.iter().any(|s| s.kind == SkipKind::Unreadable),
            "unreadable skills dir must be a skip row, not silent: {:?}",
            report.skips
        );
        assert!(report.skills.iter().all(|s| s.name != "hidden"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_unreadable() {
        let root = tempfile::tempdir().expect("tmp");
        let outside = tempfile::tempdir().expect("out");
        fs::write(outside.path().join("SKILL.md"), "secret\n").expect("write");
        let pkg = root.path().join(".agents").join("skills").join("escape");
        fs::create_dir_all(&pkg).expect("mkdir");
        std::os::unix::fs::symlink(outside.path().join("SKILL.md"), pkg.join("SKILL.md"))
            .expect("symlink");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(report.skills.is_empty());
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::Unreadable);
        assert!(report.skips[0].detail.contains("escapes"));
        assert!(report.skips[0].name.is_none());
        let msg = unknown_or_skipped_skill_message("escape", &report.skips);
        assert!(
            msg.contains("skipped skill: escape"),
            "symlink-escape skip must name the package, not unknown: {msg}"
        );
        assert!(msg.contains("unreadable"), "msg={msg}");
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }

    fn corpus_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
    }

    #[test]
    fn corpus_minimal_valid_discovers_as_extra_path() {
        let cwd = tempfile::tempdir().expect("cwd");
        let pkg = corpus_dir().join("agentskills/minimal-valid");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "minimal-valid");
        assert_eq!(report.skills[0].source, SkillSource::ExtraPath);
        assert!(report.skips.is_empty());
    }

    #[test]
    fn corpus_name_mismatch_is_skipped() {
        let cwd = tempfile::tempdir().expect("cwd");
        let parent = corpus_dir().join("agentskills/name-mismatch");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![parent.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(report.skills.is_empty());
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::NameDirectoryMismatch);
        assert_eq!(report.skips[0].name.as_deref(), Some("good-name"));
    }

    #[test]
    fn corpus_invalid_name_dir_is_parse_skip() {
        let cwd = tempfile::tempdir().expect("cwd");
        let parent = corpus_dir().join("agentskills/invalid-name");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![parent.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(report.skills.is_empty());
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::ParseError);
        assert_eq!(report.skips[0].name.as_deref(), Some("Bad_Name"));
        let why = crate::why(&report, Some("Bad_Name"), None, None);
        assert_eq!(
            why.skips.len(),
            1,
            "why JSON must find the parse skip by name"
        );
        assert_eq!(why.skips[0].name.as_deref(), Some("Bad_Name"));
    }

    #[test]
    fn corpus_collision_first_wins() {
        let cwd = tempfile::tempdir().expect("cwd");
        let a = corpus_dir().join("collision/a");
        let b = corpus_dir().join("collision/b");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![a.display().to_string(), b.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "foo");
        assert!(report.skills[0].content.contains("First path wins"));
        assert_eq!(report.skips.len(), 1);
        assert_eq!(report.skips[0].kind, SkipKind::NameCollision);
        assert!(report.skips[0].winner_path.is_some());
    }

    #[test]
    fn corpus_root_file_in_agents_tree() {
        let cwd = tempfile::tempdir().expect("cwd");
        let dest = cwd.path().join(".agents").join("skills");
        fs::create_dir_all(&dest).expect("mkdir");
        fs::copy(
            corpus_dir().join("root-file/SKILL.md"),
            dest.join("SKILL.md"),
        )
        .expect("copy");
        let report = empty_home_discover(cwd.path(), &DiscoveryOptions::default());
        assert!(report.skills.is_empty());
        assert!(
            report.skips.iter().any(|s| s.kind == SkipKind::RootFile),
            "skips={:?}",
            report.skips
        );
    }

    #[test]
    fn lowercase_skill_md_in_agents_package_loads() {
        let cwd = corpus_dir().join("lowercase-skill-md");
        let report = empty_home_discover(cwd.as_path(), &DiscoveryOptions::default());
        let skill = report
            .skills
            .iter()
            .find(|s| s.name == "lc-pack")
            .expect("lc-pack");
        assert_eq!(skill.source, SkillSource::Agents);
    }

    #[test]
    fn incumbent_claude_vendor_layout_loads() {
        let cwd = corpus_dir().join("incumbent/claude-project");
        let off = empty_home_discover(cwd.as_path(), &DiscoveryOptions::default());
        assert!(
            off.skills.iter().all(|s| s.name != "pdf-helper"),
            "claude vendor is opt-in"
        );
        let on = empty_home_discover(
            cwd.as_path(),
            &DiscoveryOptions {
                vendor_roots: vec!["claude".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        let skill = on
            .skills
            .iter()
            .find(|s| s.name == "pdf-helper")
            .expect("pdf-helper");
        assert_eq!(
            skill.source,
            SkillSource::Vendor {
                name: "claude".to_owned()
            }
        );
    }

    #[test]
    fn incumbent_vercel_skills_dir_as_extra_path() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = corpus_dir().join("incumbent/vercel-npx");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![extra.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        let skill = report
            .skills
            .iter()
            .find(|s| s.name == "deploy-hint")
            .expect("deploy-hint");
        assert_eq!(skill.source, SkillSource::ExtraPath);
    }

    #[test]
    fn extra_path_file_loads() {
        let cwd = tempfile::tempdir().expect("cwd");
        let path = cwd.path().join("one").join("SKILL.md");
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &path,
            "---\nname: one\ndescription: extra file\n---\nbody\n",
        )
        .expect("write");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![path.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].source, SkillSource::ExtraPath);
        assert_eq!(report.skills[0].name, "one");
    }

    fn env_home() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }

    #[test]
    fn home_override_clears_after_panic() {
        let home = tempfile::tempdir().expect("home");
        let leaked = home.path().to_path_buf();
        write_skill(
            &leaked.join(".agents").join("skills").join("leaked"),
            "leaked",
            "x",
        );
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_home_override(Some(leaked.clone()), || {
                panic!("boom");
            });
        }));
        assert!(panicked.is_err());
        assert_eq!(
            super::home_dir(),
            env_home(),
            "HOME override must not leak after panic"
        );
        let cwd = tempfile::tempdir().expect("cwd");
        let report = discover(cwd.path(), &DiscoveryOptions::default()).expect("discover");
        assert!(
            report.skills.iter().all(|s| s.name != "leaked"),
            "leaked home skill visible after panic: {:?}",
            report.skills
        );
    }

    #[test]
    fn home_override_restores_outer_after_nested() {
        let outer = tempfile::tempdir().expect("outer");
        let inner = tempfile::tempdir().expect("inner");
        write_skill(
            &outer.path().join(".agents").join("skills").join("outer"),
            "outer",
            "x",
        );
        write_skill(
            &inner.path().join(".agents").join("skills").join("inner"),
            "inner",
            "x",
        );
        let cwd = tempfile::tempdir().expect("cwd");
        with_home_override(Some(outer.path().to_path_buf()), || {
            with_home_override(Some(inner.path().to_path_buf()), || {
                let report = discover(cwd.path(), &DiscoveryOptions::default()).expect("inner");
                assert_eq!(report.skills.len(), 1);
                assert_eq!(report.skills[0].name, "inner");
            });
            assert_eq!(
                super::home_dir().as_deref(),
                Some(outer.path()),
                "nested override must restore outer home"
            );
            let report = discover(cwd.path(), &DiscoveryOptions::default()).expect("outer");
            assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
            assert_eq!(report.skills[0].name, "outer");
        });
        assert_eq!(super::home_dir(), env_home());
    }

    #[cfg(unix)]
    #[test]
    fn ignore_prefix_matches_canonical_walked_path() {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".agents").join("skills").join("secret"),
            "secret",
            "x",
        );
        write_skill(
            &root.path().join(".agents").join("skills").join("public"),
            "public",
            "x",
        );
        let links = tempfile::tempdir().expect("links");
        let cwd_link = links.path().join("cwd-link");
        std::os::unix::fs::symlink(root.path(), &cwd_link).expect("symlink cwd");
        let ignore = cwd_link.join(".agents").join("skills").join("secret");
        let report = empty_home_discover(
            &cwd_link,
            &DiscoveryOptions {
                ignore: vec![ignore.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(
            report.skills.iter().all(|s| s.name != "secret"),
            "ignore prefix must skip the skill after cwd canonicalize: {:?}",
            report.skills
        );
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "public");
    }

    fn project_with_secret_and_public() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tmp");
        write_skill(
            &root.path().join(".agents").join("skills").join("secret"),
            "secret",
            "x",
        );
        write_skill(
            &root.path().join(".agents").join("skills").join("public"),
            "public",
            "x",
        );
        root
    }

    fn assert_secret_ignored(report: &crate::skip::DiscoveryReport) {
        assert!(
            report.skills.iter().all(|s| s.name != "secret"),
            "secret skill must be ignored, skills={:?}",
            report.skills
        );
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "public");
    }

    #[test]
    fn ignore_relative_prefix_matches_absolute_walked_path() {
        let root = project_with_secret_and_public();
        let report = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                ignore: vec![".agents/skills/secret".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_secret_ignored(&report);
    }

    #[test]
    fn ignore_dotdot_through_missing_component_matches() {
        let root = project_with_secret_and_public();
        let ignore = root
            .path()
            .join(".agents")
            .join("skills")
            .join("missing")
            .join("..")
            .join("secret");
        let report = empty_home_discover(
            root.path(),
            &DiscoveryOptions {
                ignore: vec![ignore.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_secret_ignored(&report);
    }

    #[cfg(unix)]
    #[test]
    fn package_dir_symlink_escape_is_unreadable() {
        let root = tempfile::tempdir().expect("tmp");
        let outside = tempfile::tempdir().expect("out");
        write_skill(&outside.path().join("stolen"), "stolen", "SECRET_BODY");
        let skills = root.path().join(".agents").join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        std::os::unix::fs::symlink(outside.path().join("stolen"), skills.join("stolen"))
            .expect("symlink package dir");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(
            report.skills.iter().all(|s| s.name != "stolen"),
            "package dir symlink must not load the escaped skill: {:?}",
            report.skills
        );
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("SECRET_BODY")),
            "escaped skill body must not be loaded: {:?}",
            report.skills
        );
        let skip = report
            .skips
            .iter()
            .find(|s| s.kind == SkipKind::Unreadable)
            .expect("unreadable skip");
        assert!(
            skip.detail.contains("escapes"),
            "skip detail must name the escape: {}",
            skip.detail
        );
        let msg = unknown_or_skipped_skill_message("stolen", &report.skips);
        assert!(
            msg.contains("skipped skill: stolen"),
            "escaped package must be named, not unknown: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }

    #[cfg(unix)]
    #[test]
    fn skills_root_symlink_escape_is_unreadable() {
        let root = tempfile::tempdir().expect("tmp");
        let outside = tempfile::tempdir().expect("out");
        write_skill(&outside.path().join("stolen"), "stolen", "SECRET_BODY");
        let agents = root.path().join(".agents");
        fs::create_dir_all(&agents).expect("mkdir");
        std::os::unix::fs::symlink(outside.path(), agents.join("skills")).expect("symlink root");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(
            report.skills.iter().all(|s| s.name != "stolen"),
            "skills-root symlink must not load the escaped tree: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| { s.kind == SkipKind::Unreadable && s.detail.contains("escapes") }),
            "skills-root escape must be an unreadable skip: {:?}",
            report.skips
        );
    }

    #[cfg(unix)]
    #[test]
    fn extra_path_scan_does_not_follow_package_symlink() {
        let cwd = tempfile::tempdir().expect("cwd");
        let outside = tempfile::tempdir().expect("out");
        write_skill(&outside.path().join("stolen"), "stolen", "SECRET_BODY");
        let scan = tempfile::tempdir().expect("scan");
        std::os::unix::fs::symlink(outside.path().join("stolen"), scan.path().join("stolen"))
            .expect("symlink");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![scan.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(
            report.skills.iter().all(|s| s.name != "stolen"),
            "extra-path scan must not follow package dir symlink: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| { s.kind == SkipKind::Unreadable && s.detail.contains("escapes") }),
            "skips={:?}",
            report.skips
        );
    }

    #[cfg(unix)]
    #[test]
    fn extra_path_explicit_package_symlink_still_loads() {
        let cwd = tempfile::tempdir().expect("cwd");
        let outside = tempfile::tempdir().expect("out");
        write_skill(&outside.path().join("wanted"), "wanted", "host asked");
        let link = tempfile::tempdir().expect("link");
        let pkg = link.path().join("wanted");
        std::os::unix::fs::symlink(outside.path().join("wanted"), &pkg).expect("symlink");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "wanted");
        assert_eq!(report.skills[0].source, SkillSource::ExtraPath);
        assert!(report.skips.is_empty(), "skips={:?}", report.skips);
    }

    #[cfg(unix)]
    #[test]
    fn extra_path_skills_subdir_symlink_escape_is_unreadable() {
        let cwd = tempfile::tempdir().expect("cwd");
        let outside = tempfile::tempdir().expect("out");
        write_skill(&outside.path().join("stolen"), "stolen", "SECRET_BODY");
        let extra = tempfile::tempdir().expect("extra");
        std::os::unix::fs::symlink(outside.path(), extra.path().join("skills")).expect("symlink");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![extra.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(
            report.skills.iter().all(|s| s.name != "stolen"),
            "extra-path skills/ symlink must not load the escaped tree: {:?}",
            report.skills
        );
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("SECRET_BODY")),
            "escaped skill body must not be loaded: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| { s.kind == SkipKind::Unreadable && s.detail.contains("escapes") }),
            "skills-subdir escape must be an unreadable skip: {:?}",
            report.skips
        );
    }
}
