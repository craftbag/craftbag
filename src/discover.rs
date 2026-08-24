//! Multi-root SKILL.md discovery. First name wins.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::parse::{parse_skill, skill_name_matches_directory};
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
    /// Path prefixes to ignore (`~` expanded).
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
        Err(e) => ValidationReport {
            path: path_buf.clone(),
            ok: false,
            name: None,
            errors: vec![e.to_string()],
            skip: Some(SkillSkip {
                path: path_buf,
                name: None,
                kind: SkipKind::ParseError,
                detail: e.to_string(),
                winner_path: None,
            }),
        },
    }
}

fn discover_report(cwd: &Path, opts: &DiscoveryOptions) -> DiscoveryReport {
    let ignore = expand_path_list(&opts.ignore);
    let disabled = opts.disabled.clone();
    let mut skills = Vec::new();
    let mut skips = Vec::new();

    for dir in walk_cwd_to_git_root(cwd) {
        load_skills_from_dir(
            &dir.join(".agents").join("skills"),
            SkillSource::Agents,
            &ignore,
            &disabled,
            &[],
            &mut skills,
            &mut skips,
        );
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
        load_skills_from_dir(
            &home.join(".agents").join("skills"),
            SkillSource::Agents,
            &ignore,
            &disabled,
            &[],
            &mut skills,
            &mut skips,
        );
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

fn expand_path_list(paths: &[String]) -> Vec<PathBuf> {
    paths.iter().map(|p| expand_tilde(p)).collect()
}

fn path_is_ignored(path: &Path, ignore: &[PathBuf]) -> bool {
    if ignore.is_empty() {
        return false;
    }
    ignore.iter().any(|prefix| path.starts_with(prefix))
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
        Err(_) => return,
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md")
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

        let skill_file = path.join("SKILL.md");
        if !skill_file.is_file() {
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
                name: None,
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

#[cfg(test)]
fn with_home_override<T>(home: Option<PathBuf>, f: impl FnOnce() -> T) -> T {
    HOME_OVERRIDE.with(|o| *o.borrow_mut() = home);
    let out = f();
    HOME_OVERRIDE.with(|o| *o.borrow_mut() = None);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_VENDOR_DENYLIST, DiscoveryOptions, discover, find_skill_by_name, with_home_override,
    };
    use crate::skip::SkipKind;
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
}
