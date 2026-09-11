//! Shared SKILL.md open / parse used by the walk and extra-path.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::parse::{parse_skill, peek_frontmatter_name, skill_name_matches_directory};
use crate::skill::{SKILL_MD_MAX_BYTES, Skill};
use crate::skip::{SkillSkip, SkipKind};
use crate::source::SkillSource;

use super::DiscoveryOptions;
use super::path::{IgnorePrefix, path_is_ignored, stays_under};

#[cfg(test)]
thread_local! {
    static READ_SKILL_MD_PATHS: std::cell::RefCell<Vec<PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(super) fn take_read_skill_md_paths() -> Vec<PathBuf> {
    READ_SKILL_MD_PATHS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

pub(super) fn skip_if_dir_escapes(dir: &Path, confine: &Path, skips: &mut Vec<SkillSkip>) -> bool {
    if dir.exists() && !stays_under(dir, confine) {
        skips.push(SkillSkip {
            path: dir.to_path_buf(),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "skills directory symlink escapes walk root".to_owned(),
            winner_path: None,
            host_token: None,
        });
        return true;
    }
    false
}

pub(super) struct DirLoad<'a> {
    source: &'a SkillSource,
    ignore: &'a [IgnorePrefix],
    opts: &'a DiscoveryOptions,
    denylist: &'a [&'a str],
}

pub(super) fn dir_load<'a>(
    source: &'a SkillSource,
    ignore: &'a [IgnorePrefix],
    opts: &'a DiscoveryOptions,
    denylist: &'a [&'a str],
) -> DirLoad<'a> {
    DirLoad {
        source,
        ignore,
        opts,
        denylist,
    }
}

pub(super) fn load_skills_from_dir(
    dir: &Path,
    load: &DirLoad<'_>,
    skip: &[&Path],
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
                host_token: None,
            });
            return;
        }
    };

    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|a| a.file_name());
    for entry in entries {
        let path = entry.path();
        if skip.iter().any(|s| path == *s) {
            continue;
        }
        if !path.is_dir() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == "SKILL.md" || n == "skill.md")
                && !path_is_ignored(&path, load.ignore)
            {
                if !stays_under(&path, dir) {
                    skips.push(SkillSkip {
                        path,
                        name: None,
                        kind: SkipKind::Unreadable,
                        detail: "SKILL.md symlink escapes walk root".to_owned(),
                        winner_path: None,
                        host_token: None,
                    });
                    continue;
                }
                match read_skill_md(&path) {
                    Ok(content) => {
                        let name = peek_frontmatter_name(&content);
                        skips.push(SkillSkip {
                            path,
                            name,
                            kind: SkipKind::RootFile,
                            detail: "put the file in a named subdirectory.".to_owned(),
                            winner_path: None,
                            host_token: None,
                        });
                    }
                    Err(e) => {
                        skips.push(SkillSkip {
                            path,
                            name: None,
                            kind: SkipKind::Unreadable,
                            detail: e,
                            winner_path: None,
                            host_token: None,
                        });
                    }
                }
            }
            continue;
        }

        let skill_file = ["SKILL.md", "skill.md"]
            .into_iter()
            .map(|name| path.join(name))
            .find(|p| skill_md_inode_exists(p));
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
                host_token: None,
            });
            continue;
        }
        if skip_if_skill_md_escapes_package(&skill_file, skips) {
            continue;
        }
        try_load_skill_file(
            &skill_file,
            load.source,
            load.ignore,
            load.opts,
            load.denylist,
            skills,
            skips,
        );
    }
}

pub(super) fn skip_if_skill_md_escapes_package(
    skill_file: &Path,
    skips: &mut Vec<SkillSkip>,
) -> bool {
    if skill_md_stays_in_package(skill_file) {
        return false;
    }
    skips.push(SkillSkip {
        path: skill_file.to_path_buf(),
        name: None,
        kind: SkipKind::Unreadable,
        detail: "SKILL.md symlink escapes package root".to_owned(),
        winner_path: None,
        host_token: None,
    });
    true
}

pub(super) fn try_load_skill_file(
    skill_file: &Path,
    source: &SkillSource,
    ignore: &[IgnorePrefix],
    opts: &DiscoveryOptions,
    denylist: &[&str],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    if path_is_ignored(skill_file, ignore) {
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
                host_token: None,
            });
            return;
        }
    };
    finish_load_skill_file(skill_file, &content, source, opts, denylist, skills, skips);
}

pub(super) fn finish_load_skill_file(
    skill_file: &Path,
    content: &str,
    source: &SkillSource,
    opts: &DiscoveryOptions,
    denylist: &[&str],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    match parse_skill(content) {
        Ok(skill) => {
            finish_load_parsed_skill(skill_file, skill, source, opts, denylist, skills, skips);
        }
        Err(e) => {
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name: peek_frontmatter_name(content),
                kind: SkipKind::ParseError,
                detail: e.to_string(),
                winner_path: None,
                host_token: None,
            });
        }
    }
}

/// Skip text when `--ascii-names` / MCP `ascii_names` rejects a valid
/// Unicode name. Not invalid YAML; name the flag so the user can omit it.
pub(super) fn ascii_names_policy_detail() -> String {
    "name must be lowercase alphanumeric and hyphens only (omit --ascii-names / ascii_names to allow Unicode)".to_owned()
}

pub(super) fn finish_load_parsed_skill(
    skill_file: &Path,
    mut skill: Skill,
    source: &SkillSource,
    opts: &DiscoveryOptions,
    denylist: &[&str],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    if opts.ascii_names && !crate::parse::skill_name_is_ascii_policy(&skill.name) {
        skips.push(SkillSkip {
            path: skill_file.to_path_buf(),
            name: Some(skill.name.clone()),
            kind: SkipKind::ParseError,
            detail: ascii_names_policy_detail(),
            winner_path: None,
            host_token: None,
        });
        return;
    }
    if opts
        .disabled
        .iter()
        .any(|d| crate::parse::skill_names_equal(d, &skill.name))
    {
        return;
    }
    if denylist
        .iter()
        .any(|d| crate::parse::skill_names_equal(d, &skill.name))
    {
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
            host_token: None,
        });
        return;
    }
    if let Some(winner) = skills
        .iter()
        .find(|s| crate::parse::skill_names_equal(&s.name, &skill.name))
    {
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
            host_token: None,
        });
        return;
    }
    skill.source = source.clone();
    skill.source_path = Some(skill_file.to_path_buf());
    skills.push(skill);
}

pub(super) fn is_skill_md_filename(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n == "SKILL.md" || n == "skill.md")
}

/// Join `SKILL.md` / `skill.md` when `path` is a package directory.
///
/// Same filenames as extra-path. A collection root (no package file)
/// is an error so validate does not walk children.
pub(super) fn resolve_validate_target(path: &Path) -> Result<PathBuf, String> {
    if !skill_md_is_dir(path) {
        return Ok(path.to_path_buf());
    }
    let joined = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| path.join(name))
        .find(|p| skill_md_inode_exists(p))
        .ok_or_else(|| "directory is not a skill package (no SKILL.md)".to_owned())?;
    // Joined SKILL.md is this package unless the inode is a symlink
    // out of the directory (same as extra-path classify).
    if !skill_md_stays_in_package(&joined) {
        return Err("SKILL.md symlink escapes package root".to_owned());
    }
    Ok(joined)
}

/// True when `path` exists as any inode (regular, FIFO, socket, device, symlink).
///
/// `Path::is_file` follows links and is false for FIFO/socket/device, so a
/// package or extra-path SKILL.md of those types would be skipped before
/// [`read_skill_md`] could report unreadable.
pub(super) fn skill_md_inode_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

pub(super) fn skill_md_is_dir(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

pub(super) fn read_skill_md(path: &Path) -> Result<String, String> {
    #[cfg(test)]
    READ_SKILL_MD_PATHS.with(|c| c.borrow_mut().push(path.to_path_buf()));
    // Stat before open. `File::open` on a FIFO waits for a writer, so a
    // hostile tree can hang discover / validate.
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("SKILL.md is not a regular file".to_owned());
    }
    if meta.len() > SKILL_MD_MAX_BYTES {
        return Err(format!("SKILL.md exceeds {SKILL_MD_MAX_BYTES} bytes"));
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
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

pub(super) fn skill_md_stays_in_package(skill_file: &Path) -> bool {
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
