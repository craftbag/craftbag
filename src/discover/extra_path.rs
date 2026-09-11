//! Extra-path and user-dir classification.
//!
//! Entry point: [`load_extra_path`]. [`classify_extra_path_md`] is the
//! one-read leftover vs named-package decision. The other functions in
//! this module implement that classification.

use std::path::Path;

use crate::parse::{parse_skill, peek_frontmatter_name, skill_name_matches_directory};
use crate::skill::Skill;
use crate::skip::{SkillSkip, SkipKind};
use crate::source::SkillSource;

use super::DiscoveryOptions;
use super::host_token::{
    HostPathField, host_token_collapses_after_whitespace, path_has_line_separator,
    skip_line_separator_root, skip_unresolvable_host_path, skip_whitespace_collapse_token,
    str_has_line_separator,
};
use super::load::{
    dir_load, finish_load_parsed_skill, finish_load_skill_file, is_skill_md_filename,
    load_skills_from_dir, read_skill_md, skill_md_inode_exists, skill_md_is_dir,
    skill_md_stays_in_package, skip_if_dir_escapes, skip_if_skill_md_escapes_package,
    try_load_skill_file,
};
use super::path::{IgnorePrefix, expand_extra_path_arg, path_is_ignored, stays_under};

pub(super) fn load_extra_path(
    raw: &str,
    cwd: &Path,
    ignore: &[IgnorePrefix],
    opts: &DiscoveryOptions,
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    if str_has_line_separator(raw) {
        skip_line_separator_root(
            Path::new(&crate::sanitize_error_token(raw)),
            HostPathField::ExtraPath,
            skips,
        );
        return;
    }
    if host_token_collapses_after_whitespace(raw) {
        skip_whitespace_collapse_token(raw, HostPathField::ExtraPath, skips);
        return;
    }
    let Some(expanded) = expand_extra_path_arg(raw, cwd) else {
        return;
    };
    if path_has_line_separator(&expanded) {
        skip_line_separator_root(&expanded, HostPathField::ExtraPath, skips);
        return;
    }
    // `is_file` is false for FIFO/socket/device and symlink-to-those.
    // Still try load so `read_skill_md` can emit unreadable (and not hang).
    if is_skill_md_filename(&expanded) && skill_md_inode_exists(&expanded) && !expanded.is_dir() {
        // Host pointed at this SKILL.md inode. Same as an explicit extra-path
        // package dir symlink: do not treat a link target outside the parent
        // dir as an escaped scan.
        try_load_skill_file(
            &expanded,
            &SkillSource::ExtraPath,
            ignore,
            opts,
            &[],
            skills,
            skips,
        );
        return;
    }
    if !expanded.is_dir() {
        skip_unresolvable_host_path(&expanded, HostPathField::ExtraPath, skips);
        return;
    }
    let package_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| expanded.join(name))
        .find(|p| skill_md_inode_exists(p));
    let mut leftover_root = None;
    if let Some(skill_file) = package_md.as_ref() {
        // This extra path is that package unless we can prove the SKILL.md
        // is a loose collection root (a real name that is not this
        // directory, plus sibling packages or a skills/ tree). `.` / `..`
        // are path components, not names. Nested SKILL.md stays inside
        // the package tree. Classify once so a named package does not
        // open SKILL.md again in try_load_skill_file.
        let classified = classify_extra_path_md(&expanded, skill_file, opts.ascii_names);
        match classified {
            ExtraPathMd::Collection {
                peeked_name,
                read_err,
            } => {
                leftover_root = Some((skill_file.clone(), peeked_name, read_err));
            }
            other => {
                load_classified_extra_path_package(
                    skill_file,
                    other,
                    &SkillSource::ExtraPath,
                    ignore,
                    opts,
                    skills,
                    skips,
                );
                return;
            }
        }
    }
    let skills_subdir = expanded.join("skills");
    // Stay-under and readable before walking extra/skills/. An escaped
    // or unreadable skills/ is not a usable collection; fall back to
    // extra/. leftover extra/skills/SKILL.md that stays Package (name:
    // loose) is not collection entries. extra/skills/SKILL.md named
    // skills is a sibling package: classify once and reuse the parse.
    // A leftover SKILL.md directory stays in the extra/ walk.
    let skills_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| skills_subdir.join(name))
        .find(|p| skill_md_inode_exists(p));
    let mut extra_skills_named = false;
    let mut leftover_skills_md = None;
    if let Some(skill_file) = skills_md.as_ref() {
        let classified = classify_extra_path_md(&skills_subdir, skill_file, opts.ascii_names);
        if extra_skills_md_is_named_package(&classified) {
            extra_skills_named = true;
            load_classified_extra_path_package(
                skill_file,
                classified,
                &SkillSource::ExtraPath,
                ignore,
                opts,
                skills,
                skips,
            );
        } else {
            leftover_skills_md = Some(classified);
        }
    }
    let handle_skills = extra_skills_subdir_is_collection(&skills_subdir, &expanded, skips)
        && !extra_skills_named
        && (dir_has_child_skill_packages(&skills_subdir)
            || leftover_skills_md
                .as_ref()
                .is_some_and(leftover_extra_skills_md_is_collection_entry));
    let skip_leftover = leftover_root.as_ref().and_then(|(p, name, err)| {
        if handle_skills || !skill_md_is_dir(p) {
            skip_loose_extra_path_root_skill_md(
                p,
                &expanded,
                ignore,
                name.clone(),
                err.clone(),
                skips,
            );
            Some(p.as_path())
        } else {
            None
        }
    });
    let skip_skills_in_extra_walk =
        handle_skills || extra_skills_named || leftover_skills_md.is_some();
    if handle_skills {
        if let (Some(skill_file), Some(classified)) = (skills_md.as_ref(), leftover_skills_md) {
            skip_classified_extra_skills_leftover(
                skill_file,
                &skills_subdir,
                ignore,
                classified,
                skips,
            );
            load_skills_from_dir(
                &skills_subdir,
                &dir_load(&SkillSource::ExtraPath, ignore, opts, &[]),
                &[skill_file.as_path()],
                skills,
                skips,
            );
        } else {
            load_skills_from_dir(
                &skills_subdir,
                &dir_load(&SkillSource::ExtraPath, ignore, opts, &[]),
                &[],
                skills,
                skips,
            );
        }
    } else if let (Some(skill_file), Some(classified)) = (skills_md.as_ref(), leftover_skills_md) {
        skip_classified_extra_skills_leftover(
            skill_file,
            &skills_subdir,
            ignore,
            classified,
            skips,
        );
    }
    // Skip extra/skills here when the nested walk or leftover classify
    // already handled that tree; do not re-enter it as a sibling.
    let mut walk_skip = Vec::new();
    if let Some(p) = skip_leftover {
        walk_skip.push(p);
    }
    if skip_skills_in_extra_walk {
        walk_skip.push(skills_subdir.as_path());
    }
    load_skills_from_dir(
        &expanded,
        &dir_load(&SkillSource::ExtraPath, ignore, opts, &[]),
        &walk_skip,
        skills,
        skips,
    );
}

pub(super) fn dir_has_child_skill_packages(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        path.is_dir()
            && ["SKILL.md", "skill.md"]
                .into_iter()
                .any(|name| skill_md_inode_exists(&path.join(name)))
    })
}

pub(super) fn extra_path_has_skills_subdir(dir: &Path) -> bool {
    dir.join("skills").is_dir()
}

/// True when [`watch_dirs`] should list `dir/skills` because
/// [`discover`] would walk that collection.
///
/// Named extra-path packages keep nested `skills/` as package assets.
/// Escaped or unreadable `skills/` is not a usable collection; discover
/// falls back to `dir/` siblings and does not watch the escaped target.
pub(super) fn extra_should_watch_skills_subdir(dir: &Path, ascii_names: bool) -> bool {
    let skills_subdir = dir.join("skills");
    if !skills_subdir.is_dir() {
        return false;
    }
    if !stays_under(&skills_subdir, dir) {
        return false;
    }
    if std::fs::read_dir(&skills_subdir).is_err() {
        return false;
    }
    let skills_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| skills_subdir.join(name))
        .find(|p| skill_md_inode_exists(p));
    if let Some(skill_file) = skills_md.as_ref() {
        let classified = classify_extra_path_md(&skills_subdir, skill_file, ascii_names);
        if extra_skills_md_is_named_package(&classified) {
            return false;
        }
        if !dir_has_child_skill_packages(&skills_subdir)
            && !leftover_extra_skills_md_is_collection_entry(&classified)
        {
            return false;
        }
    } else if !dir_has_child_skill_packages(&skills_subdir) {
        return false;
    }
    let package_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|p| skill_md_inode_exists(p));
    match package_md.as_ref() {
        Some(skill_file) => extra_path_is_loose_collection(dir, skill_file, ascii_names),
        None => true,
    }
}

/// True when `user_dir/skills` is a leftover collection, not the skill
/// named `skills`. Named packages keep nested `SKILL.md` as assets.
/// A leftover that cannot be peeked (FIFO, socket, chmod) has no
/// extra/skills signal, so [`classify_extra_path_md`] is Unreadable;
/// user_dir still walks sibling packages.
pub(super) fn user_dir_skills_subdir_is_loose_collection(
    skills_subdir: &Path,
    ascii_names: bool,
) -> bool {
    let package_md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|name| skills_subdir.join(name))
        .find(|p| skill_md_inode_exists(p));
    match package_md.as_ref() {
        Some(skill_file) => match classify_extra_path_md(skills_subdir, skill_file, ascii_names) {
            ExtraPathMd::Collection { .. } | ExtraPathMd::Unreadable(_) => true,
            ExtraPathMd::Package(_) | ExtraPathMd::Parsed(_) | ExtraPathMd::ParseFailed { .. } => {
                false
            }
        },
        None => true,
    }
}

/// True when [`watch_dirs`] should list `user_dir/skills` because
/// [`discover`] walks that collection.
///
/// `user_skills_dir` is always a skills root. leftover `SKILL.md` /
/// `skill.md` is a `root_file` skip, never a named package, so a
/// matching peek must not hide `user_dir/skills` the way extra-path
/// named packages keep nested `skills/` as assets. A loadable
/// `user_dir/skills/SKILL.md` is the skill named `skills`; do not
/// watch or walk that tree as a collection.
pub(super) fn user_dir_should_watch_skills_subdir(dir: &Path, ascii_names: bool) -> bool {
    let skills_subdir = dir.join("skills");
    if !skills_subdir.is_dir() {
        return false;
    }
    if !stays_under(&skills_subdir, dir) {
        return false;
    }
    if std::fs::read_dir(&skills_subdir).is_err() {
        return false;
    }
    user_dir_skills_subdir_is_loose_collection(&skills_subdir, ascii_names)
}

/// True when `extra/skills` exists as any inode (dir, file, FIFO, socket,
/// dangling symlink). A leftover that cannot be peeked uses this as the
/// collection signal; a non-directory is not walked (FIFO hang).
pub(super) fn extra_path_has_skills_entry(dir: &Path) -> bool {
    skill_md_inode_exists(&dir.join("skills"))
}

/// extra/skills/SKILL.md that loads as the package named `skills`.
pub(super) fn extra_skills_md_is_named_package(classified: &ExtraPathMd) -> bool {
    matches!(
        classified,
        ExtraPathMd::Parsed(_) | ExtraPathMd::ParseFailed { .. }
    )
}

/// leftover extra/skills/SKILL.md that is exclusive-scan collection entries.
/// Unreadable extra/skills/SKILL.md (FIFO) and leftover Package
/// (name: loose) are not entries; extra/wanted must still load.
pub(super) fn leftover_extra_skills_md_is_collection_entry(classified: &ExtraPathMd) -> bool {
    matches!(classified, ExtraPathMd::Collection { .. })
}

/// Record leftover extra/skills/SKILL.md from classify so the extra/skills
/// walk does not open it again.
pub(super) fn skip_classified_extra_skills_leftover(
    skill_file: &Path,
    confine: &Path,
    ignore: &[IgnorePrefix],
    classified: ExtraPathMd,
    skips: &mut Vec<SkillSkip>,
) {
    match classified {
        ExtraPathMd::Collection {
            peeked_name,
            read_err,
        } => {
            skip_loose_extra_path_root_skill_md(
                skill_file,
                confine,
                ignore,
                peeked_name,
                read_err,
                skips,
            );
        }
        ExtraPathMd::Package(content) => {
            skip_loose_extra_path_root_skill_md(
                skill_file,
                confine,
                ignore,
                peek_frontmatter_name(&content),
                None,
                skips,
            );
        }
        ExtraPathMd::Unreadable(detail) => {
            skip_loose_extra_path_root_skill_md(
                skill_file,
                confine,
                ignore,
                None,
                Some(detail),
                skips,
            );
        }
        ExtraPathMd::Parsed(_) | ExtraPathMd::ParseFailed { .. } => {}
    }
}

/// True when `extra/skills/` is a readable collection that stays under
/// `extra/`. Escape and permission failures fall back to scanning
/// `extra/` so sibling packages next to `skills/` still load.
pub(super) fn extra_skills_subdir_is_collection(
    skills_subdir: &Path,
    confine: &Path,
    skips: &mut Vec<SkillSkip>,
) -> bool {
    if !skills_subdir.is_dir() {
        return false;
    }
    if skip_if_dir_escapes(skills_subdir, confine, skips) {
        return false;
    }
    match std::fs::read_dir(skills_subdir) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            skips.push(SkillSkip {
                path: skills_subdir.to_path_buf(),
                name: None,
                kind: SkipKind::Unreadable,
                detail: e.to_string(),
                winner_path: None,
                host_token: None,
            });
            false
        }
    }
}

/// One read of extra-path SKILL.md. Named packages reuse the body
/// and a successful [`parse_skill`]; leftover collection roots do
/// not load it as a package. Leftover `extra/SKILL.md` plus
/// `extra/skills/` reuses the same read for the root_file skip.
#[derive(Debug)]
pub(super) enum ExtraPathMd {
    /// Leftover collection root. Scan siblings / extra/skills.
    /// Prefetched leftover SKILL.md so skip_loose does not open it again.
    Collection {
        peeked_name: Option<String>,
        /// Set when classify already failed [`read_skill_md`].
        read_err: Option<String>,
    },
    /// Stay this extra-path package. Prefetched SKILL.md body.
    /// Load still parses (name did not match this dir, or peek
    /// failed after a leftover that is not a collection).
    Package(String),
    /// Stay this extra-path package. [`parse_skill`] already
    /// succeeded on the prefetched body.
    Parsed(Skill),
    /// Stay this extra-path package. [`parse_skill`] already
    /// failed on the prefetched body (matching peek, no extra/skills
    /// collection signal). Load must not parse again.
    ParseFailed {
        name: Option<String>,
        detail: String,
    },
    /// Stay this extra-path package. SKILL.md could not be read
    /// (FIFO, chmod, directory leftover without siblings).
    Unreadable(String),
}

/// True when this extra-path SKILL.md is a leftover collection root.
///
/// Watch helpers only need the bool. Named extra-path and
/// user_dir/skills load uses [`classify_extra_path_md`] so
/// [`try_load_skill_file`] does not open the same SKILL.md again.
pub(super) fn extra_path_is_loose_collection(
    dir: &Path,
    skill_file: &Path,
    ascii_names: bool,
) -> bool {
    matches!(
        classify_extra_path_md(dir, skill_file, ascii_names),
        ExtraPathMd::Collection { .. }
    )
}

/// Classify extra-path SKILL.md after one [`read_skill_md`].
///
/// A `skills/` subdirectory is the extra-path collection layout, same as
/// sibling package dirs. An escaped root SKILL.md is not peeked; `skills/`
/// or a sibling package dir is enough to keep scanning that tree.
/// A leftover that cannot be peeked (FIFO, socket, chmod, oversized, no
/// frontmatter name, a blank/whitespace peek, a `.` / `..` peek including
/// NFKC forms, an invalid peek that case-folds/trims to this extra-path
/// dir, a valid matching peek that `parse_skill` still rejects, or a
/// valid Unicode peek that `ascii_names` still cannot load) is the same
/// for `extra/skills/` as a directory, and for `extra/skills` as any
/// inode (file, FIFO, socket, dangling symlink). A non-directory
/// `extra/skills` is not a usable collection; fall back to `extra/` so
/// sibling packages still load. Do not open `extra/skills` as a file
/// (FIFO hang). A leftover `SKILL.md` directory also falls back to
/// sibling packages when `extra/skills` is absent. FIFO, socket, and
/// regular-file leftovers plus sibling package dirs still match a
/// named package with nested SKILL.md, so those stay a package.
pub(super) fn classify_extra_path_md(
    dir: &Path,
    skill_file: &Path,
    ascii_names: bool,
) -> ExtraPathMd {
    if !skill_md_stays_in_package(skill_file) {
        if extra_path_has_skills_subdir(dir) || dir_has_child_skill_packages(dir) {
            return ExtraPathMd::Collection {
                peeked_name: None,
                read_err: None,
            };
        }
        return ExtraPathMd::Unreadable("SKILL.md symlink escapes package root".to_owned());
    }
    let content = match read_skill_md(skill_file) {
        Ok(c) => c,
        Err(e) => {
            if extra_path_has_skills_entry(dir) {
                return ExtraPathMd::Collection {
                    peeked_name: None,
                    read_err: Some(e),
                };
            }
            // A leftover SKILL.md directory cannot be this extra-path
            // package. Scan sibling packages. FIFO, socket, and unreadable
            // regular-file leftovers still stay a package so nested
            // SKILL.md is not scanned (PR 35, PR 59).
            if skill_md_is_dir(skill_file) && dir_has_child_skill_packages(dir) {
                return ExtraPathMd::Collection {
                    peeked_name: None,
                    read_err: Some(e),
                };
            }
            return ExtraPathMd::Unreadable(e);
        }
    };
    // Missing peek name is the same miss as a leftover that cannot be
    // read: extra/skills as any inode is the collection layout (PR 71).
    let Some(name) = peek_frontmatter_name(&content) else {
        return extra_path_collection_signal_or_package(dir, content, None);
    };
    // parse_frontmatter accepts quoted whitespace (`name: "   "`), so
    // peek returns Some("   "). Load/why already treat that as nameless.
    // Same collection signal as a missing peek (PR 74).
    let normalized = crate::parse::normalize_skill_name(&name);
    if normalized.trim().is_empty() {
        return extra_path_collection_signal_or_package(dir, content, Some(name));
    }
    // `.` / `..` (including NFKC compatibility forms) never match a
    // package dir after lexical collapse, and they are not skill names.
    // Same extra/skills signal as a missing peek. Stay a package when
    // extra/skills is absent so nested SKILL.md is not scanned.
    if crate::parse::is_path_component_skill_name(&name) {
        return extra_path_collection_signal_or_package(dir, content, Some(name));
    }
    // parse_frontmatter also accepts invalid names (`DEMO`, `name: "demo "`).
    // skill_name_matches_directory case-folds and trims, so those look like
    // this extra-path package. They cannot load. peek can also return a
    // valid matching name while parse_skill still fails (missing
    // description, description over the spec cap). ascii_names makes a
    // valid Unicode peek (`café`) the same parse_error. Same extra/skills
    // signal as a missing peek. Do not use siblings: nested SKILL.md would
    // load.
    if skill_name_matches_directory(skill_file, &name) {
        if crate::parse::validate_skill_name(&name).is_err() {
            return extra_path_collection_signal_or_package(dir, content, Some(name));
        }
        if ascii_names && !crate::parse::skill_name_is_ascii_policy(&name) {
            return extra_path_collection_signal_or_package(dir, content, Some(name));
        }
        match parse_skill(&content) {
            Ok(skill) => return ExtraPathMd::Parsed(skill),
            Err(e) => {
                if extra_path_has_skills_entry(dir) {
                    return ExtraPathMd::Collection {
                        peeked_name: Some(name),
                        read_err: None,
                    };
                }
                return ExtraPathMd::ParseFailed {
                    name: Some(name),
                    detail: e.to_string(),
                };
            }
        }
    }
    if dir_has_child_skill_packages(dir) || extra_path_has_skills_subdir(dir) {
        ExtraPathMd::Collection {
            peeked_name: Some(name),
            read_err: None,
        }
    } else {
        ExtraPathMd::Package(content)
    }
}

pub(super) fn extra_path_collection_signal_or_package(
    dir: &Path,
    content: String,
    peeked_name: Option<String>,
) -> ExtraPathMd {
    if extra_path_has_skills_entry(dir) {
        ExtraPathMd::Collection {
            peeked_name,
            read_err: None,
        }
    } else {
        ExtraPathMd::Package(content)
    }
}

/// Load a classified named extra-path or user_dir/skills package
/// without a second [`read_skill_md`]. An [`ExtraPathMd::Parsed`]
/// classify also skips a second [`parse_skill`]. An
/// [`ExtraPathMd::ParseFailed`] classify skips a second
/// [`parse_skill`] the same way. Collection is handled by the caller.
pub(super) fn load_classified_extra_path_package(
    skill_file: &Path,
    classified: ExtraPathMd,
    source: &SkillSource,
    ignore: &[IgnorePrefix],
    opts: &DiscoveryOptions,
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    if skip_if_skill_md_escapes_package(skill_file, skips) {
        return;
    }
    match classified {
        ExtraPathMd::Package(content) => {
            if path_is_ignored(skill_file, ignore) {
                return;
            }
            finish_load_skill_file(skill_file, &content, source, opts, &[], skills, skips);
        }
        ExtraPathMd::Parsed(skill) => {
            if path_is_ignored(skill_file, ignore) {
                return;
            }
            finish_load_parsed_skill(skill_file, skill, source, opts, &[], skills, skips);
        }
        ExtraPathMd::ParseFailed { name, detail } => {
            if path_is_ignored(skill_file, ignore) {
                return;
            }
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name,
                kind: SkipKind::ParseError,
                detail,
                winner_path: None,
                host_token: None,
            });
        }
        ExtraPathMd::Unreadable(detail) => {
            if path_is_ignored(skill_file, ignore) {
                return;
            }
            skips.push(SkillSkip {
                path: skill_file.to_path_buf(),
                name: None,
                kind: SkipKind::Unreadable,
                detail,
                winner_path: None,
                host_token: None,
            });
        }
        ExtraPathMd::Collection { .. } => {}
    }
}

/// Record a leftover extra-path root SKILL.md from classify's prefetch.
///
/// Used when the scan target is `extra/skills/` (that walk never sees
/// `extra/SKILL.md`) and when the extra/ sibling walk must skip the
/// leftover file so it is not opened again.
pub(super) fn skip_loose_extra_path_root_skill_md(
    skill_file: &Path,
    confine: &Path,
    ignore: &[IgnorePrefix],
    peeked_name: Option<String>,
    read_err: Option<String>,
    skips: &mut Vec<SkillSkip>,
) {
    if path_is_ignored(skill_file, ignore) {
        return;
    }
    if !stays_under(skill_file, confine) {
        skips.push(SkillSkip {
            path: skill_file.to_path_buf(),
            name: None,
            kind: SkipKind::Unreadable,
            detail: "SKILL.md symlink escapes walk root".to_owned(),
            winner_path: None,
            host_token: None,
        });
        return;
    }
    if let Some(detail) = read_err {
        skips.push(SkillSkip {
            path: skill_file.to_path_buf(),
            name: None,
            kind: SkipKind::Unreadable,
            detail,
            winner_path: None,
            host_token: None,
        });
        return;
    }
    skips.push(SkillSkip {
        path: skill_file.to_path_buf(),
        name: peeked_name,
        kind: SkipKind::RootFile,
        detail: "put the file in a named subdirectory.".to_owned(),
        winner_path: None,
        host_token: None,
    });
}
