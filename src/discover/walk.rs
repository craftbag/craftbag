//! Root-matrix walk: implicit roots, watch_dirs, and validate.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::miss::SkillMiss;
use crate::parse::{
    parse_skill, peek_frontmatter_name, skill_name_matches_directory, unknown_frontmatter_keys,
};
use crate::skill::Skill;
use crate::skip::{DiscoveryReport, SkillSkip, SkipKind};
use crate::source::SkillSource;

use super::CURSOR_VENDOR_DENYLIST;
use super::DiscoveryOptions;
use super::extra_path::{
    ExtraPathMd, classify_extra_path_md, extra_should_watch_skills_subdir,
    extra_skills_md_is_named_package, extra_skills_subdir_is_collection,
    load_classified_extra_path_package, load_extra_path, skip_classified_extra_skills_leftover,
    skip_loose_extra_path_root_skill_md, user_dir_should_watch_skills_subdir,
};
use super::host_token::{
    HostPathField, component_is_whitespace_padded_dot, host_token_collapses_after_whitespace,
    nfkc_dot_path_components, one_line_error, path_has_line_separator, skip_line_separator_root,
    skip_unresolvable_host_path, skip_whitespace_collapse_token, str_has_line_separator,
};
use super::load::{
    dir_load, is_skill_md_filename, load_skills_from_dir, read_skill_md, resolve_validate_target,
    skill_md_inode_exists, skip_if_dir_escapes,
};
use super::path::{
    IgnorePrefix, expand_extra_path_arg, expand_ignore_list, expand_user_skills_dir, home_dir,
    implicit_home_already_walked, lexical_normalize, path_is_ignored, stays_under,
    walk_cwd_to_git_root,
};

/// Discover skills for `cwd` using the host-neutral root matrix.
///
/// Missing directories are not an error. Parse and IO problems become
/// [`SkillSkip`] rows. This function always returns a report.
///
/// ```
/// use craftbag::{discover, DiscoveryOptions};
///
/// let cwd = std::env::current_dir()?;
/// let report = discover(&cwd, &DiscoveryOptions::default());
/// for skill in &report.skills {
///     println!("{} {}", skill.name, skill.description);
/// }
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn discover(cwd: &Path, opts: &DiscoveryOptions) -> DiscoveryReport {
    discover_report(cwd, opts)
}

/// Existing directories (and lone extra-path `SKILL.md` files) a host
/// should watch so hot reload matches [`discover`].
///
/// Missing roots are omitted (`notify` cannot watch them). A FIFO,
/// socket, device, or regular file at a directory root is omitted
/// (`notify` can hang on a FIFO; discover does not walk it). Empty
/// `user_skills_dir` is omitted. `project` / `community` are not
/// listed (host-only). When [`DiscoveryOptions::implicit_roots`] is
/// true (the default), lists cwd-to-git `.agents` / vendor trees
/// (nearest git root first via [`walk_cwd_to_git_root`]) and
/// `$HOME/.agents` / vendor trees. When it is false, cwd-to-git and `$HOME`
/// `.agents` / vendor trees are omitted (same as [`discover`]).
/// Extra `paths` and `user_skills_dir` still load.
/// Extra-path `dir/skills` is listed only when [`discover`] would walk
/// that collection (leftover or Vercel-style). A named extra-path
/// package, or an escaped / unreadable `skills/` tree, is omitted.
/// Escaped project or home `.agents/skills` / `.{vendor}/skills`
/// (symlink out of that walk root) is omitted, same as [`discover`].
/// Host `user_skills_dir` is a skills root: leftover `SKILL.md` /
/// `skill.md` must not hide `user_dir/skills` (same collection walk
/// as extra-path leftover). [`DiscoveryOptions::ignore`] prefixes are
/// omitted (same as [`discover`]). An extra-path or `user_dir` token
/// that collapses after whitespace trim is omitted (same refuse as
/// [`discover`]).
pub fn watch_dirs(cwd: &Path, opts: &DiscoveryOptions) -> Vec<PathBuf> {
    let cwd = cwd
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(cwd));
    let ignore = expand_ignore_list(&cwd, &opts.ignore);
    let mut out = Vec::new();
    // Only existing directories. A notify watch on a missing
    // `.agents/skills` fails; hosts that want "create later" watch
    // the parent instead. A FIFO / socket / device / file is not a
    // walk root (listing a FIFO for notify can hang).
    let git_walk = if opts.implicit_roots {
        walk_cwd_to_git_root(&cwd)
    } else {
        Vec::new()
    };

    if opts.implicit_roots {
        for dir in &git_walk {
            let agents = dir.join(".agents").join("skills");
            if !path_is_ignored(&agents, &ignore) {
                push_watch_confined_dir(&mut out, agents, dir);
            }
            for name in SkillSource::VENDOR_TOKENS.iter().copied() {
                if vendor_enabled(opts, name) {
                    let vendor = dir.join(format!(".{name}")).join("skills");
                    if !path_is_ignored(&vendor, &ignore) {
                        push_watch_confined_dir(&mut out, vendor, dir);
                    }
                }
            }
        }
    }

    if !opts
        .user_skills_dir
        .as_deref()
        .and_then(|p| p.to_str())
        .is_some_and(str_has_line_separator)
    {
        if let Some(user_dir) = expand_user_skills_dir(&cwd, opts.user_skills_dir.as_deref()) {
            if !path_has_line_separator(&user_dir) {
                if !path_is_ignored(&user_dir, &ignore) {
                    push_watch_dir(&mut out, user_dir.clone());
                }
                // user_dir is always a skills root. leftover SKILL.md is a
                // root_file skip, not a named package. Watch user/skills when
                // discover would walk that collection.
                let user_skills = user_dir.join("skills");
                if user_dir_should_watch_skills_subdir(&user_dir, opts.ascii_names)
                    && !path_is_ignored(&user_skills, &ignore)
                {
                    push_watch_dir(&mut out, user_skills);
                }
            }
        }
    }

    if opts.implicit_roots {
        if let Some(home) = home_dir() {
            if !implicit_home_already_walked(&git_walk, &home) {
                let agents = home.join(".agents").join("skills");
                if !path_is_ignored(&agents, &ignore) {
                    push_watch_confined_dir(&mut out, agents, &home);
                }
                for name in SkillSource::VENDOR_TOKENS.iter().copied() {
                    if vendor_enabled(opts, name) {
                        let vendor = home.join(format!(".{name}")).join("skills");
                        if !path_is_ignored(&vendor, &ignore) {
                            push_watch_confined_dir(&mut out, vendor, &home);
                        }
                    }
                }
            }
        }
    }

    for raw in &opts.paths {
        // Same refuse as load_extra_path: trim-collapse (` /..`) or a
        // line separator must not become a notify root discover skips.
        if str_has_line_separator(raw) || host_token_collapses_after_whitespace(raw) {
            continue;
        }
        let Some(expanded) = expand_extra_path_arg(raw, &cwd) else {
            continue;
        };
        if path_has_line_separator(&expanded) {
            continue;
        }
        if path_is_ignored(&expanded, &ignore) {
            continue;
        }
        if is_skill_md_filename(&expanded) && skill_md_inode_exists(&expanded) && !expanded.is_dir()
        {
            // Discover loads a regular file (or symlink to one). A FIFO /
            // socket / device is unreadable; listing it for notify can hang.
            push_watch_file(&mut out, expanded);
            continue;
        }
        if !expanded.is_dir() {
            continue;
        }
        push_watch_dir(&mut out, expanded.clone());
        let skills_subdir = expanded.join("skills");
        if extra_should_watch_skills_subdir(&expanded, opts.ascii_names)
            && !path_is_ignored(&skills_subdir, &ignore)
        {
            push_watch_dir(&mut out, skills_subdir);
        }
    }

    out
}

/// One leftover-safe watch root per line for CLI `list --watch-dirs`
/// and MCP `skills_list format=watch`.
///
/// Path goes through [`crate::sanitize_error_token`] so a leftover
/// implicit cwd-to-git root cannot split the line (U+2028) or leak an
/// em dash. Extra-path refuse already omits a line-separator token;
/// leftover implicit paths do not. [`watch_dirs`] PathBufs stay raw
/// for notify.
pub fn format_watch_dirs(dirs: &[PathBuf]) -> String {
    let mut out = String::new();
    for dir in dirs {
        out.push_str(&crate::sanitize_error_token(&dir.display().to_string()));
        out.push('\n');
    }
    out
}

pub(super) fn push_watch_dir(out: &mut Vec<PathBuf>, p: PathBuf) {
    if p.is_dir() {
        push_watch_unique(out, p);
    }
}

pub(super) fn push_watch_confined_dir(out: &mut Vec<PathBuf>, p: PathBuf, confine: &Path) {
    if p.is_dir() && stays_under(&p, confine) {
        push_watch_unique(out, p);
    }
}

pub(super) fn push_watch_file(out: &mut Vec<PathBuf>, p: PathBuf) {
    if p.is_file() {
        push_watch_unique(out, p);
    }
}

pub(super) fn push_watch_unique(out: &mut Vec<PathBuf>, p: PathBuf) {
    if !out.iter().any(|e| e == &p) {
        out.push(p);
    }
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

impl ValidationReport {
    /// Host-branchable miss when this path did not validate.
    ///
    /// Same `{ error_kind, error }` as [`crate::unknown_or_skipped_skill`], plus
    /// `path` when the skip row is known.
    /// `error_kind` is the skip code (`parse_error`, `unreadable`,
    /// `name_directory_mismatch`). Ok reports return `None`.
    pub fn miss(&self) -> Option<SkillMiss> {
        if self.ok {
            return None;
        }
        let skip = self.skip.as_ref()?;
        let raw = self
            .errors
            .first()
            .map(String::as_str)
            .unwrap_or(skip.detail.as_str());
        Some(SkillMiss {
            error_kind: skip.kind.as_str(),
            error: crate::sanitize_error_token(raw),
            path: Some(skip.path.clone()),
            winner_path: skip.winner_path.clone(),
        })
    }
}

/// Validate a SKILL.md path: readable, parse, and name/dir match.
///
/// A package directory joins `SKILL.md` / `skill.md` (same filenames
/// as extra-path). Unknown frontmatter keys are ignored so host
/// extensions still load.
pub fn validate_path(path: &Path) -> ValidationReport {
    validate_path_with_options(path, false)
}

/// Validate a SKILL.md path or a package directory.
///
/// When `strict` is true, unknown frontmatter keys are errors. Default
/// discover and [`validate_path`] stay ignore-unknown so host extensions
/// (`triggers`, `disable_model_invocation`, …) still load.
/// A directory joins `SKILL.md` / `skill.md` like extra-path. A
/// collection root with no package file is unreadable, not a walk.
pub fn validate_path_with_options(path: &Path, strict: bool) -> ValidationReport {
    // Same refuse as extra-path: a line-separator component or a token
    // that collapses after whitespace (` /..`, `/ ..`) must not become
    // `/` / `/..` and join /SKILL.md. NFKC `wanted/evil/‥` still
    // rewrites to the wanted package below.
    if let Some(detail) = validate_path_hostile_token(path) {
        let detail = one_line_error(detail);
        let path = PathBuf::from(crate::sanitize_error_token(&path.display().to_string()));
        return ValidationReport {
            path: path.clone(),
            ok: false,
            name: None,
            errors: vec![detail.clone()],
            skip: Some(SkillSkip {
                path,
                name: None,
                kind: SkipKind::Unreadable,
                detail,
                winner_path: None,
                host_token: None,
            }),
        };
    }
    // Same NFKC `.` / `..` rewrite as extra-path, so
    // `wanted/evil/‥/SKILL.md` is the `wanted` package.
    let path = nfkc_dot_path_components(path);
    let path_buf = match resolve_validate_target(&path) {
        Ok(p) => p,
        Err(detail) => {
            let detail = one_line_error(detail);
            return ValidationReport {
                path: path.clone(),
                ok: false,
                name: None,
                errors: vec![detail.clone()],
                skip: Some(SkillSkip {
                    path,
                    name: None,
                    kind: SkipKind::Unreadable,
                    detail,
                    winner_path: None,
                    host_token: None,
                }),
            };
        }
    };
    let path = path_buf.as_path();
    if let Err(e) = std::fs::metadata(path) {
        if e.kind() == std::io::ErrorKind::NotFound {
            let shown = crate::sanitize_error_token(&path.display().to_string());
            let detail = format!(
                "path does not exist: {shown} (pass a SKILL.md file or a package directory that contains SKILL.md)"
            );
            let detail = one_line_error(detail);
            return ValidationReport {
                path: path_buf.clone(),
                ok: false,
                name: None,
                errors: vec![detail.clone()],
                skip: Some(SkillSkip {
                    path: path_buf,
                    name: None,
                    kind: SkipKind::Unreadable,
                    detail,
                    winner_path: None,
                    host_token: None,
                }),
            };
        }
    }
    let content = match read_skill_md(path) {
        Ok(c) => c,
        Err(e) => {
            let detail = one_line_error(e);
            return ValidationReport {
                path: path_buf.clone(),
                ok: false,
                name: None,
                errors: vec![detail.clone()],
                skip: Some(SkillSkip {
                    path: path_buf,
                    name: None,
                    kind: SkipKind::Unreadable,
                    detail,
                    winner_path: None,
                    host_token: None,
                }),
            };
        }
    };
    match parse_skill(&content) {
        Ok(skill) => {
            if strict {
                let unknown = unknown_frontmatter_keys(&content);
                if !unknown.is_empty() {
                    let shown: Vec<String> = unknown
                        .iter()
                        .map(|k| crate::sanitize_error_token(k))
                        .collect();
                    let detail = if shown.len() == 1 {
                        format!("unknown frontmatter key: {}", shown[0])
                    } else {
                        format!("unknown frontmatter keys: {}", shown.join(", "))
                    };
                    let err = one_line_error(ParseError::InvalidYaml(detail));
                    return ValidationReport {
                        path: path_buf.clone(),
                        ok: false,
                        name: Some(skill.name.clone()),
                        errors: vec![err.clone()],
                        skip: Some(SkillSkip {
                            path: path_buf,
                            name: Some(skill.name),
                            kind: SkipKind::ParseError,
                            detail: err,
                            winner_path: None,
                            host_token: None,
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
                    crate::sanitize_error_token(&skill.name)
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
                        host_token: None,
                    }),
                }
            }
        }
        Err(e) => {
            let name = peek_frontmatter_name(&content);
            let detail = one_line_error(e);
            ValidationReport {
                path: path_buf.clone(),
                ok: false,
                name: name.clone(),
                errors: vec![detail.clone()],
                skip: Some(SkillSkip {
                    path: path_buf,
                    name,
                    kind: SkipKind::ParseError,
                    detail,
                    winner_path: None,
                    host_token: None,
                }),
            }
        }
    }
}

pub(super) fn discover_report(cwd: &Path, opts: &DiscoveryOptions) -> DiscoveryReport {
    let cwd = cwd
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(cwd));
    let ignore = expand_ignore_list(&cwd, &opts.ignore);
    let mut skills = Vec::new();
    let mut skips = Vec::new();
    let git_walk = if opts.implicit_roots {
        walk_cwd_to_git_root(&cwd)
    } else {
        Vec::new()
    };

    if opts.implicit_roots {
        for dir in &git_walk {
            let agents = dir.join(".agents").join("skills");
            if !skip_if_dir_escapes(&agents, dir, &mut skips) {
                load_skills_from_dir(
                    &agents,
                    &dir_load(&SkillSource::Agents, &ignore, opts, &[]),
                    &[],
                    &mut skills,
                    &mut skips,
                );
            }
            load_vendor_tree(dir, opts, &ignore, &mut skills, &mut skips);
        }
    }

    if let Some(raw_path) = opts.user_skills_dir.as_deref() {
        if raw_path.to_str().is_some_and(str_has_line_separator) {
            skip_line_separator_root(raw_path, HostPathField::UserDir, &mut skips);
        } else if raw_path
            .to_str()
            .is_some_and(host_token_collapses_after_whitespace)
        {
            if let Some(raw) = raw_path.to_str() {
                skip_whitespace_collapse_token(raw, HostPathField::UserDir, &mut skips);
            }
        } else if let Some(user_dir) = expand_user_skills_dir(&cwd, Some(raw_path)) {
            if path_has_line_separator(&user_dir) {
                // Same refuse as extra-path: do not load or echo a user_dir
                // whose component would split list/why TSV or watch_dirs.
                skip_line_separator_root(&user_dir, HostPathField::UserDir, &mut skips);
            } else if !user_dir.is_dir() {
                // Host asked for this user_dir. Implicit missing `.agents`
                // stays silent; a missing or non-dir host path is a skip.
                skip_unresolvable_host_path(&user_dir, HostPathField::UserDir, &mut skips);
            } else {
                // leftover user_dir/SKILL.md is a root_file skip. extra-path
                // leftover + extra/skills is a collection; user_dir is never a
                // named package, so leftover must not hide user/skills.
                // Classify user_dir/skills/SKILL.md once (same ExtraPathMd as
                // extra-path) so leftover is RootFile, not a package
                // name_directory_mismatch, and a named skills package reuses
                // the parse. FIFO leftover is Unreadable (no extra/skills
                // signal); still walk sibling packages.
                let skills_subdir = user_dir.join("skills");
                let handle_skills =
                    extra_skills_subdir_is_collection(&skills_subdir, &user_dir, &mut skips);
                let skills_ref: &Path = &skills_subdir;
                let skip_skills = [skills_ref];
                let user_skip_skills: &[&Path] = if handle_skills { &skip_skills } else { &[] };
                load_skills_from_dir(
                    &user_dir,
                    &dir_load(&SkillSource::User, &ignore, opts, &[]),
                    user_skip_skills,
                    &mut skills,
                    &mut skips,
                );
                if handle_skills {
                    let package_md = ["SKILL.md", "skill.md"]
                        .into_iter()
                        .map(|name| skills_subdir.join(name))
                        .find(|p| skill_md_inode_exists(p));
                    if let Some(skill_file) = package_md {
                        let classified =
                            classify_extra_path_md(&skills_subdir, &skill_file, opts.ascii_names);
                        match classified {
                            ExtraPathMd::Collection {
                                peeked_name,
                                read_err,
                            } => {
                                skip_loose_extra_path_root_skill_md(
                                    &skill_file,
                                    &skills_subdir,
                                    &ignore,
                                    peeked_name,
                                    read_err,
                                    &mut skips,
                                );
                                load_skills_from_dir(
                                    &skills_subdir,
                                    &dir_load(&SkillSource::User, &ignore, opts, &[]),
                                    &[skill_file.as_path()],
                                    &mut skills,
                                    &mut skips,
                                );
                            }
                            ExtraPathMd::Unreadable(detail) => {
                                // FIFO / socket / chmod leftover has no extra/skills
                                // collection signal. user_dir is still a skills root.
                                skip_loose_extra_path_root_skill_md(
                                    &skill_file,
                                    &skills_subdir,
                                    &ignore,
                                    None,
                                    Some(detail),
                                    &mut skips,
                                );
                                load_skills_from_dir(
                                    &skills_subdir,
                                    &dir_load(&SkillSource::User, &ignore, opts, &[]),
                                    &[skill_file.as_path()],
                                    &mut skills,
                                    &mut skips,
                                );
                            }
                            other => {
                                // Package leftover (name: loose, no sibling)
                                // is extra-path skip_loose / RootFile.
                                // Only Parsed / ParseFailed is the skill
                                // named `skills`.
                                if extra_skills_md_is_named_package(&other) {
                                    load_classified_extra_path_package(
                                        &skill_file,
                                        other,
                                        &SkillSource::User,
                                        &ignore,
                                        opts,
                                        &mut skills,
                                        &mut skips,
                                    );
                                } else {
                                    skip_classified_extra_skills_leftover(
                                        &skill_file,
                                        &skills_subdir,
                                        &ignore,
                                        other,
                                        &mut skips,
                                    );
                                    load_skills_from_dir(
                                        &skills_subdir,
                                        &dir_load(&SkillSource::User, &ignore, opts, &[]),
                                        &[skill_file.as_path()],
                                        &mut skills,
                                        &mut skips,
                                    );
                                }
                            }
                        }
                    } else {
                        load_skills_from_dir(
                            &skills_subdir,
                            &dir_load(&SkillSource::User, &ignore, opts, &[]),
                            &[],
                            &mut skills,
                            &mut skips,
                        );
                    }
                }
            }
        }
    }

    if opts.implicit_roots {
        if let Some(home) = home_dir() {
            if !implicit_home_already_walked(&git_walk, &home) {
                let agents = home.join(".agents").join("skills");
                if !skip_if_dir_escapes(&agents, &home, &mut skips) {
                    load_skills_from_dir(
                        &agents,
                        &dir_load(&SkillSource::Agents, &ignore, opts, &[]),
                        &[],
                        &mut skills,
                        &mut skips,
                    );
                }
                load_vendor_tree(&home, opts, &ignore, &mut skills, &mut skips);
            }
        }
    }

    for raw in &opts.paths {
        // Empty or whitespace-only is not a path (not discover cwd).
        if raw.trim().is_empty() {
            continue;
        }
        load_extra_path(raw, &cwd, &ignore, opts, &mut skills, &mut skips);
    }

    DiscoveryReport { skills, skips }
}

pub(super) fn load_vendor_tree(
    root: &Path,
    opts: &DiscoveryOptions,
    ignore: &[IgnorePrefix],
    skills: &mut Vec<Skill>,
    skips: &mut Vec<SkillSkip>,
) {
    for name in SkillSource::VENDOR_TOKENS.iter().copied() {
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
        load_skills_from_dir(
            &dir,
            &dir_load(&source, ignore, opts, denylist),
            &[],
            skills,
            skips,
        );
    }
}

pub(super) fn vendor_enabled(opts: &DiscoveryOptions, name: &str) -> bool {
    opts.vendor_roots.iter().any(|v| v == name)
}

/// Refuse validate tokens discover already drops as extra-path.
///
/// A line-separator component (`evil\nroot`, `/..\n`) or a token that
/// collapses after whitespace (` /..`, `/ ..`) must not be rewritten
/// into `/` / `/..` and join `/SKILL.md`.
pub(super) fn validate_path_hostile_token(path: &Path) -> Option<&'static str> {
    if path.to_str().is_some_and(str_has_line_separator) || path_has_line_separator(path) {
        return Some(HostPathField::Validate.line_sep_detail());
    }
    if path
        .to_str()
        .is_some_and(host_token_collapses_after_whitespace)
    {
        return Some(HostPathField::Validate.collapse_detail());
    }
    let padded = path.components().any(|c| match c {
        Component::Normal(s) => s.to_str().is_some_and(component_is_whitespace_padded_dot),
        _ => false,
    });
    if padded {
        return Some(HostPathField::Validate.collapse_detail());
    }
    None
}
