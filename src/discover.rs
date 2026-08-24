//! Multi-root SKILL.md discovery. First name wins.

use std::cell::RefCell;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ParseError};
use crate::parse::{
    parse_skill, peek_frontmatter_name, skill_name_matches_directory, unknown_frontmatter_keys,
};
use crate::skill::{SKILL_MD_MAX_BYTES, Skill};
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
    /// Extra paths (`~` expanded). Relative paths join the discover `cwd`.
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

/// Case-insensitive skill lookup by frontmatter `name` (NFKC).
pub fn find_skill_by_name<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    if name.trim().is_empty() {
        return None;
    }
    skills
        .iter()
        .find(|s| crate::parse::skill_names_equal(&s.name, name))
}

/// Error text when `load` cannot return a skill.
///
/// A matching skip row (parse error, name/dir mismatch, unreadable
/// package) is not "unknown". Blank peeked names and the `SKILL.md`
/// parent directory (except root-file skips) are identities too.
/// Peeked `.` / `..` are path components, not skill names. The
/// message includes skip kind, package identity, and the SKILL.md
/// path so extra-path `.` / `..` (joined to discover cwd) stay
/// locatable.
pub fn unknown_or_skipped_skill_message(name: &str, skips: &[SkillSkip]) -> String {
    let want = name.trim();
    let skip = skips.iter().find(|s| s.matches_requested_name(want));
    match skip {
        Some(skip) => format!(
            "skipped skill: {} ({}) at {}: {}",
            skip_display_name(skip, want),
            skip.kind.as_str(),
            skip.path.display(),
            skip.detail
        ),
        None => format!("unknown skill: {name}"),
    }
}

fn skip_display_name<'a>(skip: &'a SkillSkip, want: &'a str) -> &'a str {
    skip.name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .or_else(|| crate::skip::skill_md_package_name(&skip.path))
        .unwrap_or(want)
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
///
/// Unknown frontmatter keys are ignored so host extensions still load.
pub fn validate_path(path: &Path) -> ValidationReport {
    validate_path_with_options(path, false)
}

/// Validate a SKILL.md path.
///
/// When `strict` is true, unknown frontmatter keys are errors. Default
/// discover and [`validate_path`] stay ignore-unknown so host extensions
/// (`triggers`, `disable_model_invocation`, …) still load.
pub fn validate_path_with_options(path: &Path, strict: bool) -> ValidationReport {
    let path_buf = path.to_path_buf();
    let content = match read_skill_md(path) {
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
            if strict {
                let unknown = unknown_frontmatter_keys(&content);
                if !unknown.is_empty() {
                    let detail = if unknown.len() == 1 {
                        format!("unknown frontmatter key: {}", unknown[0])
                    } else {
                        format!("unknown frontmatter keys: {}", unknown.join(", "))
                    };
                    let err = ParseError::InvalidYaml(detail);
                    return ValidationReport {
                        path: path_buf.clone(),
                        ok: false,
                        name: Some(skill.name.clone()),
                        errors: vec![err.to_string()],
                        skip: Some(SkillSkip {
                            path: path_buf,
                            name: Some(skill.name),
                            kind: SkipKind::ParseError,
                            detail: err.to_string(),
                            winner_path: None,
                        }),
                    };
                }
            }
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
    let cwd = cwd
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(cwd));
    let ignore = expand_ignore_list(&cwd, &opts.ignore);
    let mut skills = Vec::new();
    let mut skips = Vec::new();

    for dir in walk_cwd_to_git_root(&cwd) {
        let agents = dir.join(".agents").join("skills");
        if !skip_if_dir_escapes(&agents, &dir, &mut skips) {
            load_skills_from_dir(
                &agents,
                &SkillSource::Agents,
                &ignore,
                &opts.disabled,
                &[],
                &mut skills,
                &mut skips,
            );
        }
        load_vendor_tree(&dir, opts, &ignore, &opts.disabled, &mut skills, &mut skips);
    }

    if let Some(user_dir) = &opts.user_skills_dir {
        load_skills_from_dir(
            user_dir,
            &SkillSource::User,
            &ignore,
            &opts.disabled,
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
                &SkillSource::Agents,
                &ignore,
                &opts.disabled,
                &[],
                &mut skills,
                &mut skips,
            );
        }
        load_vendor_tree(
            &home,
            opts,
            &ignore,
            &opts.disabled,
            &mut skills,
            &mut skips,
        );
    }

    for raw in &opts.paths {
        load_extra_path(raw, &cwd, &ignore, &opts.disabled, &mut skills, &mut skips);
    }

    DiscoveryReport { skills, skips }
}

fn load_vendor_tree(
    root: &Path,
    opts: &DiscoveryOptions,
    ignore: &[IgnorePrefix],
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
        let source = SkillSource::Vendor {
            name: name.to_owned(),
        };
        load_skills_from_dir(&dir, &source, ignore, disabled, denylist, skills, skips);
    }
}

fn vendor_enabled(opts: &DiscoveryOptions, name: &str) -> bool {
    opts.vendor_roots.iter().any(|v| v == name)
}

fn load_extra_path(
    raw: &str,
    cwd: &Path,
    ignore: &[IgnorePrefix],
    disabled: &[String],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    let expanded = expand_tilde(raw);
    // Relative extra paths join `cwd`, same as ignore prefixes. `.` / `..`
    // then keep a real package dir name after lexical collapse.
    let expanded = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    if expanded.is_file()
        && expanded
            .file_name()
            .is_some_and(|n| n == "SKILL.md" || n == "skill.md")
    {
        try_load_skill_file(
            &expanded,
            &SkillSource::ExtraPath,
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
        // This extra path is that package unless we can prove the SKILL.md
        // is a loose collection root (a real name that is not this
        // directory, plus sibling packages). `.` / `..` are path
        // components, not names. Nested SKILL.md stays inside the
        // package tree.
        if !extra_path_is_loose_collection(&expanded, &skill_file) {
            try_load_skill_file(
                &skill_file,
                &SkillSource::ExtraPath,
                ignore,
                disabled,
                &[],
                skills,
                skips,
            );
            return;
        }
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
        &SkillSource::ExtraPath,
        ignore,
        disabled,
        &[],
        skills,
        skips,
    );
}

fn dir_has_child_skill_packages(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        path.is_dir()
            && ["SKILL.md", "skill.md"]
                .into_iter()
                .any(|name| path.join(name).is_file())
    })
}

/// True when this extra-path SKILL.md is a loose root next to sibling packages.
fn extra_path_is_loose_collection(dir: &Path, skill_file: &Path) -> bool {
    if !skill_md_stays_in_package(skill_file) {
        return false;
    }
    let Ok(content) = read_skill_md(skill_file) else {
        return false;
    };
    let Some(name) = peek_frontmatter_name(&content) else {
        return false;
    };
    let name = name.trim();
    // `.` / `..` never match a package dir after lexical collapse, and
    // they are not skill names. Treat them like a missing peek name.
    if name.is_empty()
        || name == "."
        || name == ".."
        || skill_name_matches_directory(skill_file, name)
    {
        return false;
    }
    dir_has_child_skill_packages(dir)
}

fn walk_cwd_to_git_root(cwd: &Path) -> Vec<PathBuf> {
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

struct IgnorePrefix {
    lexical: PathBuf,
    canonical: Option<PathBuf>,
}

fn expand_ignore_list(cwd: &Path, paths: &[String]) -> Vec<IgnorePrefix> {
    paths
        .iter()
        .map(|p| {
            let expanded = expand_tilde(p);
            let joined = if expanded.is_absolute() {
                expanded
            } else {
                cwd.join(expanded)
            };
            let lexical = lexical_normalize(&joined);
            let canonical = joined
                .canonicalize()
                .ok()
                .or_else(|| lexical.canonicalize().ok());
            IgnorePrefix { lexical, canonical }
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

fn path_is_ignored(path: &Path, ignore: &[IgnorePrefix]) -> bool {
    if ignore.is_empty() {
        return false;
    }
    ignore
        .iter()
        .any(|prefix| path_has_ignore_prefix(path, prefix))
}

fn path_has_ignore_prefix(path: &Path, prefix: &IgnorePrefix) -> bool {
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
    source: &SkillSource,
    ignore: &[IgnorePrefix],
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
                let name = match read_skill_md(&path) {
                    Ok(content) => peek_frontmatter_name(&content),
                    Err(_) => None,
                };
                skips.push(SkillSkip {
                    path,
                    name,
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
            source,
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
    source: &SkillSource,
    ignore: &[IgnorePrefix],
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

    let content = match read_skill_md(skill_file) {
        Ok(c) => c,
        Err(e) => {
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name: None,
                kind: SkipKind::Unreadable,
                detail: e,
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
            skill.source = source.clone();
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

fn read_skill_md(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if let Ok(meta) = file.metadata() {
        if meta.len() > SKILL_MD_MAX_BYTES {
            return Err(format!("SKILL.md exceeds {SKILL_MD_MAX_BYTES} bytes"));
        }
    }
    let mut buf = String::new();
    let n = file
        .take(SKILL_MD_MAX_BYTES.saturating_add(1))
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    if n as u64 > SKILL_MD_MAX_BYTES {
        return Err(format!("SKILL.md exceeds {SKILL_MD_MAX_BYTES} bytes"));
    }
    Ok(buf)
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
        unknown_or_skipped_skill_message, validate_path, validate_path_with_options,
        with_home_override,
    };
    use crate::skill::SKILL_MD_MAX_BYTES;
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
        assert!(
            msg.contains("/tmp/Bad_Name/SKILL.md"),
            "load must name the skip path: {msg}"
        );
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
    fn load_miss_blank_peeked_name_uses_package_dir() {
        let skip = SkillSkip {
            path: PathBuf::from("/tmp/demo/skill.md"),
            name: Some("  ".to_owned()),
            kind: SkipKind::ParseError,
            detail: "name must be lowercase alphanumeric and hyphens only".to_owned(),
            winner_path: None,
        };
        let msg = unknown_or_skipped_skill_message("demo", &[skip]);
        assert!(
            msg.contains("skipped skill: demo"),
            "blank peek name must not hide the package dir: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }

    #[test]
    fn load_and_why_agree_on_named_and_nameless_collision() {
        let named = SkillSkip {
            path: PathBuf::from("/tmp/other/SKILL.md"),
            name: Some("alpha".to_owned()),
            kind: SkipKind::ParseError,
            detail: "invalid YAML".to_owned(),
            winner_path: None,
        };
        let nameless = SkillSkip {
            path: PathBuf::from("/tmp/alpha/SKILL.md"),
            name: None,
            kind: SkipKind::ParseError,
            detail: "missing required field: name".to_owned(),
            winner_path: None,
        };
        let skips = [named, nameless];
        let load_alpha = unknown_or_skipped_skill_message("alpha", &skips);
        let why_alpha = crate::why(
            &crate::skip::DiscoveryReport {
                skills: vec![],
                skips: skips.to_vec(),
            },
            Some("alpha"),
            None,
            None,
        );
        assert!(
            load_alpha.contains("skipped skill"),
            "load alpha must not be unknown: {load_alpha}"
        );
        assert!(why_alpha.unknown_skill_message().is_none());
        assert_eq!(why_alpha.skips.len(), 2);
        let load_other = unknown_or_skipped_skill_message("other", &skips);
        let why_other = crate::why(
            &crate::skip::DiscoveryReport {
                skills: vec![],
                skips: skips.to_vec(),
            },
            Some("other"),
            None,
            None,
        );
        assert!(
            load_other.contains("skipped skill: alpha"),
            "named skip in other/ must not look missing: {load_other}"
        );
        assert!(why_other.unknown_skill_message().is_none());
        assert_eq!(why_other.skips.len(), 1);
    }

    #[test]
    fn padded_frontmatter_name_discover_is_skipped_not_unknown() {
        let root = tempfile::tempdir().expect("tmp");
        let pkg = root.path().join(".agents").join("skills").join("demo");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("SKILL.md"),
            "---\nname: \"demo \"\ndescription: trailing space in name\n---\nbody\n",
        )
        .expect("write");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(report.skills.is_empty(), "skills={:?}", report.skills);
        assert_eq!(report.skips.len(), 1, "skips={:?}", report.skips);
        assert_eq!(report.skips[0].kind, SkipKind::ParseError);
        let msg = unknown_or_skipped_skill_message("demo", &report.skips);
        assert!(
            msg.contains("skipped skill"),
            "padded peek name must still be the demo package: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("demo"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn lowercase_skill_md_nameless_parse_matches_case_fold() {
        let cwd = tempfile::tempdir().expect("cwd");
        let pkg = cwd.path().join("extra").join("Demo");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("skill.md"),
            "---\ndescription: no name\n---\nbody\n",
        )
        .expect("write");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![cwd.path().join("extra").display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(report.skills.is_empty(), "skills={:?}", report.skills);
        assert_eq!(report.skips.len(), 1, "skips={:?}", report.skips);
        let msg = unknown_or_skipped_skill_message("demo", &report.skips);
        assert!(
            msg.contains("skipped skill: Demo"),
            "lowercase skill.md parent dir must case-fold: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("DEMO"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert!(why.unknown_skill_message().is_none());
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
        assert_eq!(
            report.skips[0].name.as_deref(),
            Some("loose"),
            "root_file skip must keep the frontmatter name so load/why do not call it unknown"
        );
        let msg = unknown_or_skipped_skill_message("loose", &report.skips);
        assert!(
            msg.contains("skipped skill: loose"),
            "load must name the skipped root file: {msg}"
        );
        assert!(msg.contains("root_file"), "msg={msg}");
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        assert_eq!(
            unknown_or_skipped_skill_message("skills", &report.skips),
            "unknown skill: skills",
            "skills-root parent dir is still not a package identity"
        );
        let why = crate::why(&report, Some("loose"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert_eq!(why.skips[0].kind, SkipKind::RootFile);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn nameless_root_file_stays_unknown_on_load_and_why() {
        let root = tempfile::tempdir().expect("tmp");
        let skills = root.path().join(".agents").join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        fs::write(
            skills.join("SKILL.md"),
            "---\ndescription: no name\n---\nbody\n",
        )
        .expect("write");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(report.skills.is_empty(), "skills={:?}", report.skills);
        assert_eq!(report.skips.len(), 1, "skips={:?}", report.skips);
        assert_eq!(report.skips[0].kind, SkipKind::RootFile);
        assert_eq!(report.skips[0].name, None);
        assert_eq!(
            unknown_or_skipped_skill_message("skills", &report.skips),
            "unknown skill: skills"
        );
        let why = crate::why(&report, Some("skills"), None, None);
        assert!(why.skips.is_empty());
        assert_eq!(
            why.unknown_skill_message().as_deref(),
            Some("unknown skill: skills")
        );
    }

    #[test]
    fn root_file_same_name_as_package_does_not_block_load() {
        let root = tempfile::tempdir().expect("tmp");
        let skills = root.path().join(".agents").join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        fs::write(
            skills.join("SKILL.md"),
            "---\nname: demo\ndescription: loose\n---\nloose body\n",
        )
        .expect("write");
        write_skill(&skills.join("demo"), "demo", "package body");
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["demo"],
            "package must still load: skills={:?} skips={:?}",
            report.skills,
            report.skips
        );
        assert_eq!(report.skills[0].content.trim(), "package body");
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::RootFile && s.name.as_deref() == Some("demo")),
            "loose file must stay a named root_file skip: {:?}",
            report.skips
        );
        let loaded = find_skill_by_name(&report.skills, "demo");
        assert!(loaded.is_some(), "load demo must hit the package");
        let load_msg = unknown_or_skipped_skill_message("demo", &report.skips);
        let why = crate::why(&report, Some("demo"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert_eq!(why.skips.len(), 1, "why skips={:?}", why.skips);
        assert_eq!(why.skips[0].kind, SkipKind::RootFile);
        assert!(
            why.unknown_skill_message().is_none(),
            "why must not call a loaded name unknown"
        );
        assert!(
            loaded.is_some(),
            "load/why disagree: load={load_msg} why_unknown={:?}",
            why.unknown_skill_message()
        );
    }

    #[test]
    fn extra_path_skills_subdir_root_file_and_package_same_name() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let skills = extra.path().join("skills");
        fs::create_dir_all(&skills).expect("mkdir");
        fs::write(
            skills.join("SKILL.md"),
            "---\nname: demo\ndescription: loose\n---\nloose\n",
        )
        .expect("write");
        write_skill(&skills.join("demo"), "demo", "from extra");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![extra.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["demo"],
            "extra-path skills/ package must load: {:?}",
            report
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::RootFile && s.name.as_deref() == Some("demo")),
            "skips={:?}",
            report.skips
        );
        let why = crate::why(&report, Some("DEMO"), None, None);
        assert_eq!(why.loaded.len(), 1);
        assert_eq!(why.skips.len(), 1);
        assert!(why.unknown_skill_message().is_none());
        assert!(find_skill_by_name(&report.skills, "demo").is_some());
    }

    #[test]
    fn extra_path_dir_loose_file_does_not_hide_same_name_package() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        fs::write(
            extra.path().join("SKILL.md"),
            "---\nname: demo\ndescription: loose\n---\nloose\n",
        )
        .expect("write");
        write_skill(&extra.path().join("demo"), "demo", "package");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![extra.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["demo"],
            "child package must not vanish behind a loose SKILL.md: skills={:?} skips={:?}",
            report.skills,
            report.skips
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::RootFile && s.name.as_deref() == Some("demo")),
            "loose extra-path SKILL.md must be root_file, not a package: {:?}",
            report.skips
        );
        let why = crate::why(&report, Some("demo"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert!(
            why.skips.iter().any(|s| s.kind == SkipKind::RootFile),
            "why skips={:?}",
            why.skips
        );
        assert!(why.unknown_skill_message().is_none());
        assert!(find_skill_by_name(&report.skills, "demo").is_some());
    }

    #[test]
    fn extra_path_single_package_dir_still_loads() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("demo");
        write_skill(&pkg, "demo", "only package");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "demo");
        assert!(report.skips.is_empty(), "skips={:?}", report.skips);
    }

    #[test]
    fn extra_path_named_package_does_not_scan_nested_skill() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "PACKAGE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["wanted"],
            "named extra-path package must load; nested SKILL.md is not a sibling: {:?}",
            report
        );
        assert_eq!(report.skills[0].content.trim(), "PACKAGE_BODY");
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("NESTED_SECRET")),
            "nested skill body must not load: {:?}",
            report.skills
        );
        assert!(
            report.skips.iter().all(|s| s.kind != SkipKind::RootFile),
            "wanted/SKILL.md must not become a root_file skip: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "wanted").is_some());
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let why = crate::why(&report, Some("wanted"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn extra_path_parentdir_is_same_named_package() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "PACKAGE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let via_parent = pkg.join("evil").join("..");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![via_parent.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["wanted"],
            "wanted/evil/.. must be the wanted package, not a collection scan: {:?}",
            report
        );
        assert_eq!(report.skills[0].content.trim(), "PACKAGE_BODY");
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("NESTED_SECRET")),
            "nested skill body must not load from wanted/evil/..: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .all(|s| s.kind != SkipKind::RootFile && s.kind != SkipKind::NameDirectoryMismatch),
            "wanted/evil/.. must not become root_file or name_directory_mismatch: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "wanted").is_some());
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let why = crate::why(&report, Some("wanted"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn extra_path_dot_cwd_is_same_named_package() {
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "PACKAGE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let report = empty_home_discover(
            &pkg,
            &DiscoveryOptions {
                paths: vec![".".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["wanted"],
            "extra-path `.` with cwd in the package must be that package: {:?}",
            report
        );
        assert_eq!(report.skills[0].content.trim(), "PACKAGE_BODY");
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("NESTED_SECRET")),
            "nested skill body must not load from extra-path `.`: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .all(|s| s.kind != SkipKind::RootFile && s.kind != SkipKind::NameDirectoryMismatch),
            "extra-path `.` must not become root_file or name_directory_mismatch: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "wanted").is_some());
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let why = crate::why(&report, Some("wanted"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn extra_path_dotdot_cwd_is_same_named_package() {
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "PACKAGE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let report = empty_home_discover(
            &pkg.join("evil"),
            &DiscoveryOptions {
                paths: vec!["..".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["wanted"],
            "extra-path `..` from a child of the package must be that package: {:?}",
            report
        );
        assert_eq!(report.skills[0].content.trim(), "PACKAGE_BODY");
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("NESTED_SECRET")),
            "nested skill body must not load from extra-path `..`: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .all(|s| s.kind != SkipKind::RootFile && s.kind != SkipKind::NameDirectoryMismatch),
            "extra-path `..` must not become root_file or name_directory_mismatch: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "wanted").is_some());
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let why = crate::why(&report, Some("wanted"), None, None);
        assert_eq!(why.loaded.len(), 1, "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn extra_path_parentdir_skill_md_file_is_same_named_package() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "FILE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let via_parent_file = pkg.join("evil").join("..").join("SKILL.md");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![via_parent_file.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["wanted"],
            "wanted/evil/../SKILL.md must load as wanted: {:?}",
            report
        );
        assert_eq!(report.skills[0].content.trim(), "FILE_BODY");
        assert!(
            report
                .skills
                .iter()
                .all(|s| !s.content.contains("NESTED_SECRET")),
            "file extra-path must not scan nested SKILL.md: {:?}",
            report.skills
        );
        assert!(
            report
                .skips
                .iter()
                .all(|s| s.kind != SkipKind::NameDirectoryMismatch),
            "wanted/evil/../SKILL.md must match directory wanted: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
    }

    #[test]
    fn extra_path_nameless_parse_error_does_not_scan_nested_skill() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("SKILL.md"),
            "---\ndescription: no name\n---\nPACKAGE_BODY\n",
        )
        .expect("write");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(
            report.skills.is_empty(),
            "nameless extra-path SKILL.md must not load nested SKILL.md: {:?}",
            report
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::ParseError && s.path.ends_with("wanted/SKILL.md")),
            "nameless SKILL.md must stay a parse_error package skip: {:?}",
            report.skips
        );
        assert!(
            report.skips.iter().all(|s| s.kind != SkipKind::RootFile),
            "nameless wanted/SKILL.md must not become a root_file skip: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
        assert!(
            msg.contains("skipped skill: wanted"),
            "nameless extra-path package must not look missing: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("wanted"), None, None);
        assert!(why.loaded.is_empty(), "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
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

    #[test]
    fn extra_path_unicode_package_why_matches_folded_name() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        write_skill(&extra.path().join("перевод"), "перевод", "docs");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![extra.path().display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(
            report
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["перевод"],
            "extra-path Unicode dir must load: {:?}",
            report
        );
        assert!(find_skill_by_name(&report.skills, "ПЕРЕВОД").is_some());
        let why = crate::why(&report, Some("ПЕРЕВОД"), None, None);
        assert_eq!(
            why.loaded.len(),
            1,
            "why must find the Unicode package under the folded query: {:?}",
            why
        );
        assert_eq!(why.loaded[0].name, "перевод");
        assert!(why.unknown_skill_message().is_none());
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
            report
                .skips
                .iter()
                .any(|s| { s.kind == SkipKind::RootFile && s.name.as_deref() == Some("loose") }),
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

    fn write_sized_skill(dir: &std::path::Path, name: &str, total_bytes: usize) {
        fs::create_dir_all(dir).expect("mkdir");
        let header = format!("---\nname: {name}\ndescription: {name} skill\n---\n");
        assert!(
            total_bytes >= header.len(),
            "total_bytes={} smaller than header {}",
            total_bytes,
            header.len()
        );
        let mut body = header;
        body.extend(std::iter::repeat_n('x', total_bytes - body.len()));
        fs::write(dir.join("SKILL.md"), body).expect("write");
    }

    #[test]
    fn oversized_skill_md_is_unreadable_skip() {
        let root = tempfile::tempdir().expect("tmp");
        write_sized_skill(
            &root.path().join(".agents").join("skills").join("huge"),
            "huge",
            SKILL_MD_MAX_BYTES as usize + 1,
        );
        write_skill(
            &root.path().join(".agents").join("skills").join("ok-one"),
            "ok-one",
            "keep",
        );
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert!(
            report.skills.iter().all(|s| s.name != "huge"),
            "oversized SKILL.md must not load: {:?}",
            report.skills
        );
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "ok-one");
        let skip = report
            .skips
            .iter()
            .find(|s| s.kind == SkipKind::Unreadable)
            .expect("unreadable skip");
        assert!(
            skip.detail.contains(&SKILL_MD_MAX_BYTES.to_string()),
            "skip detail must name the byte cap: {}",
            skip.detail
        );
        let msg = unknown_or_skipped_skill_message("huge", &report.skips);
        assert!(
            msg.contains("skipped skill: huge"),
            "oversized package must be named, not unknown: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("huge"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn skill_md_at_byte_cap_still_loads() {
        let root = tempfile::tempdir().expect("tmp");
        write_sized_skill(
            &root.path().join(".agents").join("skills").join("padded"),
            "padded",
            SKILL_MD_MAX_BYTES as usize,
        );
        let report = empty_home_discover(root.path(), &DiscoveryOptions::default());
        assert_eq!(report.skills.len(), 1, "skills={:?}", report.skills);
        assert_eq!(report.skills[0].name, "padded");
        assert!(report.skips.is_empty(), "skips={:?}", report.skips);
    }

    #[test]
    fn validate_path_rejects_oversized_skill_md() {
        let root = tempfile::tempdir().expect("tmp");
        let pkg = root.path().join("huge");
        write_sized_skill(&pkg, "huge", SKILL_MD_MAX_BYTES as usize + 1);
        let report = validate_path(&pkg.join("SKILL.md"));
        assert!(!report.ok);
        assert!(report.name.is_none());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains(&SKILL_MD_MAX_BYTES.to_string())),
            "errors={:?}",
            report.errors
        );
        let skip = report.skip.expect("skip");
        assert_eq!(skip.kind, SkipKind::Unreadable);
        assert!(skip.detail.contains(&SKILL_MD_MAX_BYTES.to_string()));
    }

    fn write_unknown_key_skill(dir: &std::path::Path) {
        fs::create_dir_all(dir).expect("mkdir");
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: demo\ndescription: d\nmade_up_field: x\n---\nbody\n",
        )
        .expect("write");
    }

    #[test]
    fn validate_path_ignores_unknown_frontmatter_key() {
        let root = tempfile::tempdir().expect("tmp");
        let pkg = root.path().join("demo");
        write_unknown_key_skill(&pkg);
        let report = validate_path(&pkg.join("SKILL.md"));
        assert!(
            report.ok,
            "default validate ignores unknown keys: {:?}",
            report.errors
        );
        assert_eq!(report.name.as_deref(), Some("demo"));
    }

    #[test]
    fn validate_path_strict_rejects_unknown_frontmatter_key() {
        let root = tempfile::tempdir().expect("tmp");
        let pkg = root.path().join("demo");
        write_unknown_key_skill(&pkg);
        let report = validate_path_with_options(&pkg.join("SKILL.md"), true);
        assert!(!report.ok, "strict must reject made_up_field");
        assert!(
            report.errors.iter().any(|e| e.contains("made_up_field")),
            "errors={:?}",
            report.errors
        );
        let skip = report.skip.expect("skip");
        assert_eq!(skip.kind, SkipKind::ParseError);
        assert_eq!(skip.code(), "parse_error");
    }

    #[test]
    fn validate_path_strict_accepts_corpus_and_host_extensions() {
        let corpus = corpus_dir().join("agentskills/minimal-valid/SKILL.md");
        let report = validate_path_with_options(&corpus, true);
        assert!(report.ok, "corpus must pass --strict: {:?}", report.errors);

        let root = tempfile::tempdir().expect("tmp");
        let pkg = root.path().join("hosty");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("SKILL.md"),
            concat!(
                "---\n",
                "name: hosty\n",
                "description: host extensions\n",
                "license: MIT\n",
                "compatibility: rust\n",
                "allowed-tools: Read\n",
                "triggers:\n",
                "  - hosty\n",
                "user_invocable: true\n",
                "disable_model_invocation: false\n",
                "argument-hint: name\n",
                "when-to-use: when testing\n",
                "metadata:\n",
                "  author: craftbag\n",
                "---\nbody\n",
            ),
        )
        .expect("write");
        let report = validate_path_with_options(&pkg.join("SKILL.md"), true);
        assert!(
            report.ok,
            "known fields and host extensions must pass --strict: {:?}",
            report.errors
        );
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

    #[test]
    fn extra_path_oversized_skill_md_does_not_scan_nested_skill() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        fs::create_dir_all(&pkg).expect("mkdir");
        let mut huge = String::from("---\nname: wanted\ndescription: huge\n---\n");
        huge.push_str(&"x".repeat(SKILL_MD_MAX_BYTES as usize + 1));
        fs::write(pkg.join("SKILL.md"), huge).expect("write");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        assert!(
            report.skills.is_empty(),
            "oversized extra-path package must not load nested SKILL.md: {:?}",
            report
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::Unreadable && s.path.ends_with("wanted/SKILL.md")),
            "oversized SKILL.md must stay an unreadable package skip: {:?}",
            report.skips
        );
        assert!(
            report.skips.iter().all(|s| s.kind != SkipKind::RootFile),
            "oversized wanted/SKILL.md must not become a root_file skip: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
        assert!(
            msg.contains("skipped skill: wanted"),
            "oversized extra-path package must not look missing: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("wanted"), None, None);
        assert!(why.loaded.is_empty(), "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn extra_path_unreadable_skill_md_does_not_scan_nested_skill() {
        let cwd = tempfile::tempdir().expect("cwd");
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        write_skill(&pkg, "wanted", "PACKAGE_BODY");
        write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
        let skill_file = pkg.join("SKILL.md");
        let mut perms = fs::metadata(&skill_file).expect("meta").permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o000);
        fs::set_permissions(&skill_file, perms).expect("chmod");
        let report = empty_home_discover(
            cwd.path(),
            &DiscoveryOptions {
                paths: vec![pkg.display().to_string()],
                ..DiscoveryOptions::default()
            },
        );
        let _ = fs::set_permissions(&skill_file, fs::Permissions::from_mode(0o644));
        assert!(
            report.skills.is_empty(),
            "unreadable extra-path package must not load nested SKILL.md: {:?}",
            report
        );
        assert!(
            report
                .skips
                .iter()
                .any(|s| s.kind == SkipKind::Unreadable && s.path.ends_with("wanted/SKILL.md")),
            "unreadable SKILL.md must stay an unreadable package skip: {:?}",
            report.skips
        );
        assert!(
            report.skips.iter().all(|s| s.kind != SkipKind::RootFile),
            "unreadable wanted/SKILL.md must not become a root_file skip: {:?}",
            report.skips
        );
        assert!(find_skill_by_name(&report.skills, "evil").is_none());
        let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
        assert!(
            msg.contains("skipped skill: wanted"),
            "unreadable extra-path package must not look missing: {msg}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("wanted"), None, None);
        assert!(why.loaded.is_empty(), "why loaded={:?}", why.loaded);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn extra_path_dot_or_dotdot_frontmatter_name_does_not_scan_nested() {
        for peeked in [".", ".."] {
            let cwd = tempfile::tempdir().expect("cwd");
            let extra = tempfile::tempdir().expect("extra");
            let pkg = extra.path().join("wanted");
            fs::create_dir_all(&pkg).expect("mkdir");
            fs::write(
                pkg.join("SKILL.md"),
                format!("---\nname: {peeked}\ndescription: path-like name\n---\nPACKAGE_BODY\n"),
            )
            .expect("write");
            write_skill(&pkg.join("evil"), "evil", "NESTED_SECRET");
            let report = empty_home_discover(
                cwd.path(),
                &DiscoveryOptions {
                    paths: vec![pkg.display().to_string()],
                    ..DiscoveryOptions::default()
                },
            );
            assert!(
                report.skills.is_empty(),
                "name `{peeked}` must not prove a loose collection: {:?}",
                report
            );
            assert!(
                report
                    .skills
                    .iter()
                    .all(|s| !s.content.contains("NESTED_SECRET")),
                "nested skill body must not load for name `{peeked}`: {:?}",
                report.skills
            );
            assert!(
                report.skips.iter().any(|s| {
                    s.kind == SkipKind::ParseError && s.path.ends_with("wanted/SKILL.md")
                }),
                "name `{peeked}` must stay a parse_error package skip: {:?}",
                report.skips
            );
            assert!(
                report.skips.iter().all(|s| s.kind != SkipKind::RootFile),
                "wanted/SKILL.md with name `{peeked}` must not become a root_file skip: {:?}",
                report.skips
            );
            assert!(find_skill_by_name(&report.skills, "evil").is_none());
            let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
            assert!(
                msg.contains("skipped skill: wanted"),
                "peeked `{peeked}` is a path component, not the package name: {msg}"
            );
            assert!(
                !msg.contains(&format!("skipped skill: {peeked}")),
                "load must not present peeked `{peeked}` as the skill name: {msg}"
            );
            assert!(msg.contains("parse_error"), "msg={msg}");
            let skip_path = report.skips[0].path.display().to_string();
            assert!(
                msg.contains(&skip_path),
                "load must name the SKILL.md path: msg={msg} path={skip_path}"
            );
            assert!(!msg.contains("unknown skill"), "msg={msg}");
            let why = crate::why(&report, Some("wanted"), None, None);
            assert!(why.loaded.is_empty(), "why loaded={:?}", why.loaded);
            assert!(
                why.unknown_skill_message().is_none(),
                "why wanted must find the package skip, not unknown: {:?}",
                why.unknown_skill_message()
            );
        }
    }

    #[test]
    fn load_extra_path_dot_parse_skip_includes_joined_path() {
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("SKILL.md"),
            "---\ndescription: no name\n---\nPACKAGE_BODY\n",
        )
        .expect("write");
        let report = empty_home_discover(
            &pkg,
            &DiscoveryOptions {
                paths: vec![".".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skips.len(), 1, "skips={:?}", report.skips);
        assert_eq!(report.skips[0].kind, SkipKind::ParseError);
        let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
        assert!(
            msg.contains("skipped skill: wanted"),
            "extra-path `.` package must stay wanted: {msg}"
        );
        assert!(msg.contains("parse_error"), "msg={msg}");
        let skip_path = report.skips[0].path.display().to_string();
        assert!(
            skip_path.contains("wanted"),
            "skip path must be the joined discover cwd, not raw `.`: {skip_path}"
        );
        assert!(
            msg.contains(&skip_path),
            "load/MCP must name the joined SKILL.md after extra-path `.`: msg={msg} path={skip_path}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
        let why = crate::why(&report, Some("wanted"), None, None);
        assert_eq!(why.skips.len(), 1);
        assert!(why.unknown_skill_message().is_none());
    }

    #[test]
    fn load_extra_path_dotdot_parse_skip_includes_joined_path() {
        let extra = tempfile::tempdir().expect("extra");
        let pkg = extra.path().join("wanted");
        fs::create_dir_all(&pkg).expect("mkdir");
        fs::write(
            pkg.join("SKILL.md"),
            "---\ndescription: no name\n---\nPACKAGE_BODY\n",
        )
        .expect("write");
        let child = pkg.join("evil");
        fs::create_dir_all(&child).expect("mkdir");
        let report = empty_home_discover(
            &child,
            &DiscoveryOptions {
                paths: vec!["..".to_owned()],
                ..DiscoveryOptions::default()
            },
        );
        assert_eq!(report.skips.len(), 1, "skips={:?}", report.skips);
        assert_eq!(report.skips[0].kind, SkipKind::ParseError);
        let msg = unknown_or_skipped_skill_message("wanted", &report.skips);
        assert!(
            msg.contains("skipped skill: wanted"),
            "extra-path `..` package must stay wanted: {msg}"
        );
        assert!(msg.contains("parse_error"), "msg={msg}");
        let skip_path = report.skips[0].path.display().to_string();
        assert!(
            skip_path.contains("wanted"),
            "skip path must be the joined discover cwd, not raw `..`: {skip_path}"
        );
        assert!(
            msg.contains(&skip_path),
            "load/MCP must name the joined SKILL.md after extra-path `..`: msg={msg} path={skip_path}"
        );
        assert!(!msg.contains("unknown skill"), "msg={msg}");
    }
}
