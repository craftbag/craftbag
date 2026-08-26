//! Hand-rolled SKILL.md frontmatter parser. Do not add `serde_yaml`.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use unicode_normalization::UnicodeNormalization;

use crate::error::ParseError;
use crate::skill::{
    SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS, SKILL_NAME_MAX_CHARS, Skill,
};

/// Hyphen YAML spelling and the snake name hosts peel from parse errors.
/// Parse arms look up this table; never pass the raw YAML `key` into
/// [`require_bool_yaml`].
const HYPHEN_BOOL_KEYS: &[(&str, &str)] = &[
    ("user-invocable", "user_invocable"),
    ("disable-model-invocation", "disable_model_invocation"),
];

/// Official agentskills fields plus host extensions this crate parses.
pub(crate) fn is_known_frontmatter_key(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "description"
            | "license"
            | "compatibility"
            | "metadata"
            | "allowed-tools"
            | "allowed_tools"
            | "triggers"
            | "argument-hint"
            | "argument_hint"
            | "when-to-use"
            | "when_to_use"
    ) || canonical_bool_yaml_key(key).is_some()
}

fn canonical_bool_yaml_key(key: &str) -> Option<&'static str> {
    for &(hyphen, snake) in HYPHEN_BOOL_KEYS {
        if key == hyphen || key == snake {
            return Some(snake);
        }
    }
    None
}

/// Top-level frontmatter keys that parse ignores (not known or host extensions).
pub(crate) fn unknown_frontmatter_keys(content: &str) -> Vec<String> {
    let Some(yaml) = frontmatter_yaml(content) else {
        return Vec::new();
    };
    let mut unknown = Vec::new();
    for line in yaml.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, _)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || is_known_frontmatter_key(key) {
            continue;
        }
        if !unknown.iter().any(|k| k == key) {
            unknown.push(key.to_owned());
        }
    }
    unknown
}

fn frontmatter_yaml(content: &str) -> Option<&str> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..].trim_start_matches(['\r', '\n']);
    let close_pos = after_open.find("\n---")?;
    Some(&after_open[..close_pos])
}

/// Parse a SKILL.md file's content into a [`Skill`].
///
/// Required fields and name rules follow
/// [agentskills.io](https://agentskills.io/specification). Host extensions
/// (`triggers`, `user-invocable`, …) are accepted as optional frontmatter.
///
/// `source` defaults to [`crate::SkillSource::Agents`]; callers override it
/// from the discovery root.
pub fn parse_skill(content: &str) -> Result<Skill, ParseError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(ParseError::MissingFrontmatter);
    }

    let after_open = &trimmed[3..].trim_start_matches(['\r', '\n']);
    let close_pos = after_open
        .find("\n---")
        .ok_or(ParseError::MissingFrontmatter)?;

    let yaml_block = &after_open[..close_pos];
    let body_start = close_pos + 4;
    let body = after_open[body_start..].trim_start_matches(['\r', '\n']);

    let mut skill = parse_frontmatter(yaml_block)?;
    skill.content = body.to_owned();
    skill.name = normalize_skill_name(&skill.name);

    validate_skill_name(&skill.name)?;
    if skill.description.is_empty() {
        return Err(ParseError::MissingField("description".to_owned()));
    }
    if skill.description.chars().count() > SKILL_DESCRIPTION_MAX_CHARS {
        return Err(ParseError::InvalidYaml(format!(
            "description exceeds {SKILL_DESCRIPTION_MAX_CHARS} characters"
        )));
    }
    if let Some(c) = &skill.compatibility {
        if c.chars().count() > SKILL_COMPATIBILITY_MAX_CHARS {
            return Err(ParseError::InvalidYaml(format!(
                "compatibility exceeds {SKILL_COMPATIBILITY_MAX_CHARS} characters"
            )));
        }
    }

    Ok(skill)
}

/// NFKC form of a skill name (skills-ref / agentskills Unicode policy).
pub fn normalize_skill_name(name: &str) -> String {
    name.nfkc().collect()
}

/// True when two names are the same package after NFKC and case fold.
///
/// Directory names on APFS may be NFD; frontmatter is usually NFC.
pub fn skill_names_equal(a: &str, b: &str) -> bool {
    let a = normalize_skill_name(a.trim());
    let b = normalize_skill_name(b.trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.to_lowercase() == b.to_lowercase()
}

/// True when `name` is empty or `.` / `..` after NFKC and trim.
///
/// Extra-path treats those as path components, not skill names. Compatibility
/// forms (fullwidth `.`, two-dot leader) must not unlock a nested scan.
pub(crate) fn is_path_component_skill_name(name: &str) -> bool {
    let n = normalize_skill_name(name);
    let n = n.trim();
    n.is_empty() || n == "." || n == ".."
}

fn is_skill_name_char(c: char) -> bool {
    if c == '-' {
        return true;
    }
    c.is_alphanumeric() && !c.is_uppercase()
}

/// Validate agentskills.io `name` field rules after NFKC.
pub fn validate_skill_name(name: &str) -> Result<(), ParseError> {
    let name = normalize_skill_name(name);
    let len = name.chars().count();
    if len == 0 || len > SKILL_NAME_MAX_CHARS {
        return Err(ParseError::InvalidYaml(format!(
            "name must be 1–{SKILL_NAME_MAX_CHARS} characters"
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(ParseError::InvalidYaml(
            "name must not start or end with a hyphen".to_owned(),
        ));
    }
    if name.contains("--") {
        return Err(ParseError::InvalidYaml(
            "name must not contain consecutive hyphens".to_owned(),
        ));
    }
    if !name.chars().all(is_skill_name_char) {
        return Err(ParseError::InvalidYaml(
            "name must be lowercase alphanumeric (Unicode, NFKC) and hyphens only".to_owned(),
        ));
    }
    Ok(())
}

/// True when `name` is only `a-z0-9-`.
///
/// Call after [`validate_skill_name`]. Hyphen edges and consecutive
/// hyphens are already rejected there.
pub fn skill_name_is_ascii_policy(name: &str) -> bool {
    let name = normalize_skill_name(name);
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Frontmatter `name` when the field parsed, even if the skill is invalid.
pub(crate) fn peek_frontmatter_name(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..].trim_start_matches(['\r', '\n']);
    let close_pos = after_open.find("\n---")?;
    let yaml_block = &after_open[..close_pos];
    if let Ok(skill) = parse_frontmatter(yaml_block) {
        return Some(skill.name);
    }
    scan_frontmatter_name(yaml_block)
}

fn scan_frontmatter_name(yaml: &str) -> Option<String> {
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let raw_value = strip_yaml_inline_comment(value);
        let value = raw_value.trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            return None;
        }
        return Some(value.to_owned());
    }
    None
}

/// Parent directory name of a `SKILL.md` path after stripping `.` / `..`.
///
/// `wanted/./SKILL.md` and `wanted/other/../SKILL.md` are the `wanted`
/// package, same as `wanted/SKILL.md`. NFKC compatibility dots (`．`,
/// `‥`, …) are the same components, matching extra-path and ignore.
pub(crate) fn skill_md_package_dir_name(skill_md: &Path) -> Option<&str> {
    let parent = skill_md.parent()?;
    let mut stack: Vec<&std::ffi::OsStr> = Vec::new();
    for c in parent.components() {
        match c {
            Component::Normal(s) => {
                if let Some(text) = s.to_str() {
                    let n = normalize_skill_name(text);
                    let n = n.trim();
                    if n == "." {
                        continue;
                    }
                    if n == ".." {
                        let _ = stack.pop();
                        continue;
                    }
                }
                stack.push(s);
            }
            Component::ParentDir => {
                let _ = stack.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => stack.clear(),
        }
    }
    stack.last().and_then(|s| s.to_str())
}

/// True when the parent directory name of `skill_md` matches `name`.
pub fn skill_name_matches_directory(skill_md: &Path, name: &str) -> bool {
    match skill_md_package_dir_name(skill_md) {
        Some(dir) => skill_names_equal(dir, name),
        None => false,
    }
}

/// Parse YAML frontmatter into a skill (body empty until filled by [`parse_skill`]).
pub(crate) fn parse_frontmatter(yaml: &str) -> Result<Skill, ParseError> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut triggers: Vec<String> = Vec::new();
    let mut license: Option<String> = None;
    let mut compatibility: Option<String> = None;
    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    let mut allowed_tools: Option<String> = None;
    let mut user_invocable = true;
    let mut disable_model_invocation = false;
    let mut argument_hint: Option<String> = None;
    let mut when_to_use: Option<String> = None;

    let mut in_triggers = false;
    let mut in_metadata = false;

    let mut lines = yaml.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if in_metadata {
            let is_indented = line.starts_with(' ') || line.starts_with('\t');
            if is_indented {
                if let Some((k, v)) = trimmed.split_once(':') {
                    let k = k.trim();
                    let v = strip_yaml_inline_comment(v)
                        .trim_matches('"')
                        .trim_matches('\'');
                    if !k.is_empty() {
                        metadata.insert(k.to_owned(), v.to_owned());
                    }
                }
                continue;
            }
            in_metadata = false;
        }

        if trimmed.starts_with("- ") && in_triggers {
            let item = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            let item = strip_yaml_inline_comment(item)
                .trim_matches('"')
                .trim_matches('\'');
            if !item.is_empty() {
                triggers.push(item.to_owned());
            }
            continue;
        }

        in_triggers = false;

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let raw_value = strip_yaml_inline_comment(value);
            let value = raw_value.trim_matches('"').trim_matches('\'');

            if let Some(style) = yaml_block_scalar_style(raw_value) {
                let block = take_yaml_block_scalar(&mut lines, style);
                if let Some(canon) = canonical_bool_yaml_key(key) {
                    assign_parsed_bool(
                        canon,
                        require_bool_yaml(canon, &block)?,
                        &mut user_invocable,
                        &mut disable_model_invocation,
                    );
                    continue;
                }
                if block.is_empty() {
                    return Err(ParseError::InvalidYaml(format!(
                        "{key} block scalar is empty"
                    )));
                }
                match key {
                    "description" => description = Some(block),
                    "license" => license = Some(block),
                    "compatibility" => compatibility = Some(block),
                    "allowed-tools" | "allowed_tools" => allowed_tools = Some(block),
                    "argument-hint" | "argument_hint" => argument_hint = Some(block),
                    "when-to-use" | "when_to_use" => when_to_use = Some(block),
                    "name" => {
                        return Err(ParseError::InvalidYaml(
                            "name must be a single-line scalar".to_owned(),
                        ));
                    }
                    _ => {}
                }
                continue;
            }

            match key {
                "name" => {
                    if value.is_empty() {
                        return Err(ParseError::InvalidYaml("name value is empty".to_owned()));
                    }
                    name = Some(value.to_owned());
                }
                "description" => {
                    if value.is_empty() {
                        return Err(ParseError::InvalidYaml(
                            "description value is empty".to_owned(),
                        ));
                    }
                    description = Some(value.to_owned());
                }
                "triggers" => {
                    if value.is_empty() {
                        in_triggers = true;
                    } else {
                        push_inline_triggers(&mut triggers, value);
                    }
                }
                "license" if !value.is_empty() => {
                    license = Some(value.to_owned());
                }
                "compatibility" if !value.is_empty() => {
                    compatibility = Some(value.to_owned());
                }
                "allowed-tools" | "allowed_tools" if !value.is_empty() => {
                    allowed_tools = Some(value.to_owned());
                }
                "metadata" if value.is_empty() => {
                    in_metadata = true;
                }
                "argument-hint" | "argument_hint" if !value.is_empty() => {
                    argument_hint = Some(value.to_owned());
                }
                "when-to-use" | "when_to_use" if !value.is_empty() => {
                    when_to_use = Some(value.to_owned());
                }
                _ => {
                    if let Some(canon) = canonical_bool_yaml_key(key) {
                        assign_parsed_bool(
                            canon,
                            require_bool_yaml(canon, value)?,
                            &mut user_invocable,
                            &mut disable_model_invocation,
                        );
                    }
                }
            }
        } else {
            return Err(ParseError::InvalidYaml(format!(
                "expected `key: value`, got: {trimmed}"
            )));
        }
    }

    let name = name.ok_or_else(|| ParseError::MissingField("name".to_owned()))?;
    let description =
        description.ok_or_else(|| ParseError::MissingField("description".to_owned()))?;

    let mut skill = Skill::new(name, description, "");
    skill.triggers = triggers;
    skill.license = license;
    skill.compatibility = compatibility;
    skill.metadata = metadata;
    skill.allowed_tools = allowed_tools;
    skill.user_invocable = user_invocable;
    skill.disable_model_invocation = disable_model_invocation;
    skill.argument_hint = argument_hint;
    skill.when_to_use = when_to_use;
    Ok(skill)
}

fn yaml_block_scalar_style(value: &str) -> Option<char> {
    let v = value.trim();
    let mut chars = v.chars();
    let first = chars.next()?;
    if first != '>' && first != '|' {
        return None;
    }
    if chars.all(|c| c == '-' || c == '+') {
        Some(first)
    } else {
        None
    }
}

fn take_yaml_block_scalar<'a, I>(lines: &mut std::iter::Peekable<I>, style: char) -> String
where
    I: Iterator<Item = &'a str>,
{
    let mut parts: Vec<String> = Vec::new();
    while let Some(next) = lines.peek() {
        if next.is_empty() {
            parts.push(String::new());
            lines.next();
            continue;
        }
        if !(next.starts_with(' ') || next.starts_with('\t')) {
            break;
        }
        let Some(raw) = lines.next() else {
            break;
        };
        parts.push(raw.trim().to_owned());
    }
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if style == '|' {
        parts.join("\n")
    } else {
        let mut out = String::new();
        let mut para: Vec<&str> = Vec::new();
        for p in &parts {
            if p.is_empty() {
                if !para.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&para.join(" "));
                    para.clear();
                }
            } else {
                para.push(p.as_str());
            }
        }
        if !para.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&para.join(" "));
        }
        out
    }
}

fn push_inline_triggers(triggers: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    for part in inner.split(',') {
        let item = part.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if !item.is_empty() {
            triggers.push(item.to_owned());
        }
    }
}

fn assign_parsed_bool(
    canon: &str,
    parsed: bool,
    user_invocable: &mut bool,
    disable_model_invocation: &mut bool,
) {
    match canon {
        "user_invocable" => *user_invocable = parsed,
        "disable_model_invocation" => *disable_model_invocation = parsed,
        _ => {}
    }
}

fn parse_bool_yaml(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Present bool key with empty / null / garbage is an error, not the
/// omitted default. Same rule as MCP present-null.
/// `canon` is the table snake name, never the raw YAML key.
fn require_bool_yaml(canon: &str, value: &str) -> Result<bool, ParseError> {
    parse_bool_yaml(value).ok_or_else(|| {
        let shown = crate::sanitize_error_token(value);
        if shown.trim().is_empty() {
            ParseError::InvalidYaml(format!("{canon} value is empty"))
        } else {
            ParseError::InvalidYaml(format!("{canon} must be a boolean, got: {shown}"))
        }
    })
}

fn strip_yaml_inline_comment(raw: &str) -> &str {
    let s = raw.trim();
    if s.starts_with('#') {
        return "";
    }
    if let Some(q @ (b'"' | b'\'')) = s.as_bytes().first().copied() {
        let rest = &s.as_bytes()[1..];
        let mut i = 0;
        while i < rest.len() {
            if q == b'"' && rest[i] == b'\\' && i + 1 < rest.len() {
                i += 2;
                continue;
            }
            if rest[i] == q {
                return &s[..=i + 1];
            }
            i += 1;
        }
        return s;
    }
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return s[..i].trim_end();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{
        HYPHEN_BOOL_KEYS, is_known_frontmatter_key, parse_frontmatter, parse_skill,
        peek_frontmatter_name, skill_name_matches_directory, unknown_frontmatter_keys,
        validate_skill_name,
    };
    use crate::error::ParseError;
    use crate::skill::SKILL_DESCRIPTION_MAX_CHARS;
    use crate::source::SkillSource;

    #[test]
    fn parse_skill_full_frontmatter() {
        let input = "\
---
name: git-workflow
description: Git branching conventions
triggers:
  - git
  - commit
---
## Rules
Always rebase.
";
        let skill = parse_skill(input).expect("should parse");
        assert_eq!(skill.name, "git-workflow");
        assert_eq!(skill.description, "Git branching conventions");
        assert_eq!(skill.triggers, vec!["git", "commit"]);
        assert!(skill.content.contains("Always rebase."));
        assert_eq!(skill.source, SkillSource::Agents);
    }

    #[test]
    fn parse_skill_no_triggers_defaults_to_empty() {
        let input = "\
---
name: style-guide
description: Code style rules
---
Use 4-space indent.
";
        let skill = parse_skill(input).expect("should parse");
        assert_eq!(skill.name, "style-guide");
        assert!(skill.triggers.is_empty());
    }

    #[test]
    fn parse_skill_missing_frontmatter() {
        let input = "# Just markdown\nNo frontmatter here.";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingFrontmatter));
        assert_eq!(err.to_string(), "missing YAML frontmatter");
    }

    #[test]
    fn parse_skill_missing_name() {
        let input = "\
---
description: Something useful
---
Body.
";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(ref f) if f == "name"));
    }

    #[test]
    fn parse_skill_missing_description() {
        let input = "\
---
name: my-skill
---
Body.
";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(ref f) if f == "description"));
    }

    #[test]
    fn parse_skill_preserves_content_body() {
        let body = "Line 1\n\n## Heading\n\nParagraph with **bold**.";
        let input = format!("---\nname: test\ndescription: test skill\n---\n{body}");
        let skill = parse_skill(&input).expect("should parse");
        assert_eq!(skill.content, body);
    }

    #[test]
    fn parse_skill_folded_multiline_description() {
        let input = "\
---
name: demo-skill
description: >
  When the user says BANANAPHONE, you MUST reply with exactly the single
  token SKILL_HIT and nothing else of substance.
---
# Demo Skill
Reply with exactly: SKILL_HIT
";
        let skill = parse_skill(input).expect("folded description should parse");
        assert_eq!(skill.name, "demo-skill");
        assert!(
            skill.description.contains("BANANAPHONE") && skill.description.contains("SKILL_HIT"),
            "description={}",
            skill.description
        );
        assert!(!skill.description.contains('>'));
        assert!(skill.content.contains("SKILL_HIT"));
    }

    #[test]
    fn parse_skill_literal_multiline_description() {
        let input = "\
---
name: lit-skill
description: |
  line one
  line two
---
Body.
";
        let skill = parse_skill(input).expect("literal description should parse");
        assert_eq!(skill.description, "line one\nline two");
    }

    #[test]
    fn parse_skill_single_inline_trigger() {
        let input = "\
---
name: ci
description: CI helpers
triggers: deploy
---
Content.
";
        let skill = parse_skill(input).expect("should parse");
        assert_eq!(skill.triggers, vec!["deploy"]);
    }

    #[test]
    fn parse_skill_inline_triggers_split_on_commas() {
        let input = "\
---
name: pr-ci-own
description: Own CI until green
triggers: CI, checks, pull request green, own CI
---
Content.
";
        let skill = parse_skill(input).expect("should parse");
        assert_eq!(
            skill.triggers,
            vec!["CI", "checks", "pull request green", "own CI"]
        );
    }

    #[test]
    fn parse_skill_quoted_values() {
        let input = "\
---
name: \"quoted-name\"
description: 'single quoted desc'
triggers:
  - \"quoted-trigger\"
---
Body.
";
        let skill = parse_skill(input).expect("should parse");
        assert_eq!(skill.name, "quoted-name");
        assert_eq!(skill.description, "single quoted desc");
        assert_eq!(skill.triggers, vec!["quoted-trigger"]);
    }

    #[test]
    fn parse_skill_hash_comment_line_in_frontmatter_loads() {
        let input = "\
---
# pdf helpers
name: pdf-processing
description: Extract and summarize PDF files
---
# PDF
Use pdftotext.
";
        let skill = parse_skill(input).expect("hash comment line must not skip the skill");
        assert_eq!(skill.name, "pdf-processing");
        assert_eq!(skill.description, "Extract and summarize PDF files");
        assert!(skill.content.contains("pdftotext"));
    }

    #[test]
    fn parse_skill_triggers_comment_only_line_loads() {
        let input = "\
---
name: pdf-processing
description: Extract and summarize PDF files
triggers:
  # activation phrases
  - pdf
  - invoice
---
# PDF
Use pdftotext.
";
        let skill = parse_skill(input).expect("comment-only line under triggers must not skip");
        assert_eq!(skill.name, "pdf-processing");
        assert_eq!(skill.triggers, vec!["pdf", "invoice"]);
    }

    #[test]
    fn parse_skill_name_trailing_comment_does_not_fail() {
        let input = "\
---
name: pdf-processing # pack
description: Extract and summarize PDF files
---
# PDF
Use pdftotext.
";
        let skill = parse_skill(input).expect("trailing comment on name must not fail validation");
        assert_eq!(skill.name, "pdf-processing");
    }

    #[test]
    fn parse_skill_unclosed_frontmatter() {
        let input = "---\nname: test\ndescription: test\nNo closing delimiter.";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingFrontmatter));
    }

    #[test]
    fn validate_skill_name_rejects_invalid() {
        assert!(validate_skill_name("ok-name").is_ok());
        assert!(validate_skill_name("a").is_ok());
        assert!(validate_skill_name("-leading").is_err());
        assert!(validate_skill_name("trailing-").is_err());
        assert!(validate_skill_name("has--double").is_err());
        assert!(validate_skill_name("Upper").is_err());
        assert!(validate_skill_name("under_score").is_err());
        assert!(validate_skill_name("").is_err());
        let long = "a".repeat(crate::skill::SKILL_NAME_MAX_CHARS + 1);
        assert!(validate_skill_name(&long).is_err());
    }

    #[test]
    fn validate_skill_name_accepts_unicode_lowercase() {
        assert!(validate_skill_name("перевод").is_ok());
        assert!(validate_skill_name("技能").is_ok());
        assert!(validate_skill_name("übersicht").is_ok());
        assert!(validate_skill_name("пере-вод").is_ok());
        assert!(validate_skill_name("Перевод").is_err());
    }

    #[test]
    fn skill_name_is_ascii_policy_rejects_unicode() {
        assert!(super::skill_name_is_ascii_policy("ok-name"));
        assert!(super::skill_name_is_ascii_policy("a1"));
        assert!(!super::skill_name_is_ascii_policy("café"));
        assert!(!super::skill_name_is_ascii_policy("перевод"));
        assert!(!super::skill_name_is_ascii_policy(""));
    }

    #[test]
    fn skill_names_equal_nfkc_and_case() {
        assert!(super::skill_names_equal("перевод", "ПЕРЕВОД"));
        assert!(super::skill_names_equal("cafe", "cafe"));
        assert!(super::skill_names_equal("é", "e\u{0301}"));
        assert!(
            super::skill_names_equal("ᾼ", "ᾳ"),
            "Greek titlecase and lowercase are one identity after case fold"
        );
        assert!(super::validate_skill_name("ᾼ-pack").is_ok());
        assert!(super::validate_skill_name("ᾳ-pack").is_ok());
        assert!(!super::skill_names_equal("перевод", "other"));
        assert!(!super::skill_names_equal(".", "wanted"));
        assert!(super::skill_names_equal("demo\u{00A0}", "demo"));
        assert!(super::skill_names_equal("demo\u{3000}", "demo"));
        assert!(
            super::skill_names_equal("．", "."),
            "fullwidth full stop NFKC-equals `.`"
        );
        assert!(
            super::skill_names_equal("‥", ".."),
            "two-dot leader NFKC-equals `..`"
        );
        assert!(super::is_path_component_skill_name("."));
        assert!(super::is_path_component_skill_name(".."));
        assert!(
            super::is_path_component_skill_name("．"),
            "fullwidth full stop is a path component after NFKC"
        );
        assert!(
            super::is_path_component_skill_name("‥"),
            "two-dot leader is a path component after NFKC"
        );
        assert!(
            super::is_path_component_skill_name("․"),
            "one-dot leader is a path component after NFKC"
        );
        assert!(
            super::is_path_component_skill_name("﹒"),
            "small full stop is a path component after NFKC"
        );
        assert!(
            super::is_path_component_skill_name("︰"),
            "vertical two-dot leader is a path component after NFKC"
        );
        assert!(super::is_path_component_skill_name("．．"));
        assert!(super::is_path_component_skill_name("․․"));
        assert!(super::is_path_component_skill_name("\u{00A0}"));
        assert!(!super::is_path_component_skill_name("wanted"));
        assert!(!super::is_path_component_skill_name("evil"));
    }

    #[test]
    fn parse_skill_stores_nfkc_unicode_name() {
        let input = "---\nname: перевод\ndescription: docs\n---\nBody.\n";
        let skill = parse_skill(input).expect("unicode name");
        assert_eq!(skill.name, "перевод");
    }

    #[test]
    fn skill_name_matches_directory_nfkc() {
        use std::path::Path;
        assert!(skill_name_matches_directory(
            Path::new("/tmp/перевод/SKILL.md"),
            "перевод"
        ));
        assert!(skill_name_matches_directory(
            Path::new("/tmp/перевод/SKILL.md"),
            "ПЕРЕВОД"
        ));
    }

    #[test]
    fn skill_name_matches_directory_nfkc_dot_components() {
        use std::path::Path;
        assert!(
            skill_name_matches_directory(Path::new("wanted/./SKILL.md"), "wanted"),
            "ASCII `.` must collapse like extra-path"
        );
        assert!(
            skill_name_matches_directory(Path::new("wanted/other/../SKILL.md"), "wanted"),
            "ASCII `..` must collapse like extra-path"
        );
        assert!(
            skill_name_matches_directory(Path::new("wanted/．/SKILL.md"), "wanted"),
            "fullwidth `.` must collapse, not become the package name"
        );
        assert!(
            skill_name_matches_directory(Path::new("wanted/evil/‥/SKILL.md"), "wanted"),
            "two-dot leader must collapse like extra-path `wanted/evil/..`"
        );
        assert!(
            skill_name_matches_directory(Path::new("wanted/evil/︰/SKILL.md"), "wanted"),
            "vertical two-dot leader must collapse like `..`"
        );
        assert!(
            skill_name_matches_directory(Path::new("wanted/․/SKILL.md"), "wanted"),
            "one-dot leader must collapse like `.`"
        );
        assert!(
            !skill_name_matches_directory(Path::new("wanted/．/SKILL.md"), "．"),
            "NFKC `.` is a path component, not a skill name"
        );
    }

    #[test]
    fn parse_skill_rejects_description_over_1024() {
        let desc = "x".repeat(SKILL_DESCRIPTION_MAX_CHARS + 1);
        let input = format!("---\nname: too-long\ndescription: {desc}\n---\nBody.\n");
        let err = parse_skill(&input).unwrap_err();
        assert!(
            matches!(err, ParseError::InvalidYaml(ref m) if m.contains("description")),
            "{err}"
        );
    }

    #[test]
    fn unknown_frontmatter_keys_finds_made_up_field() {
        let input = "---\nname: demo\ndescription: d\nmade_up_field: x\n---\nbody\n";
        assert_eq!(unknown_frontmatter_keys(input), ["made_up_field"]);
        assert!(is_known_frontmatter_key("triggers"));
        assert!(is_known_frontmatter_key("disable_model_invocation"));
        assert!(!is_known_frontmatter_key("made_up_field"));
        let known = "\
---
name: hosty
description: d
license:
compatibility: rust
allowed-tools: Read
triggers:
  - hosty
user_invocable: true
disable_model_invocation: false
argument-hint: name
when-to-use: when testing
metadata:
  author: craftbag
---
body
";
        assert!(
            unknown_frontmatter_keys(known).is_empty(),
            "keys={:?}",
            unknown_frontmatter_keys(known)
        );
    }

    #[test]
    fn parse_frontmatter_is_crate_visible() {
        let skill = parse_frontmatter("name: demo\ndescription: d\n").expect("fm");
        assert_eq!(skill.name, "demo");
        assert!(skill.content.is_empty());
    }

    #[test]
    fn peek_frontmatter_name_survives_invalid_name() {
        let input = "\
---
name: Bad_Name
description: Invalid agentskills name (uppercase and underscore)
---
Should fail parse.
";
        assert_eq!(peek_frontmatter_name(input).as_deref(), Some("Bad_Name"));
        assert!(peek_frontmatter_name("# no frontmatter\n").is_none());
        let missing_desc = "---\nname: only-name\n---\nBody.\n";
        assert_eq!(
            peek_frontmatter_name(missing_desc).as_deref(),
            Some("only-name")
        );
    }

    #[test]
    fn skill_name_matches_directory_strips_curdir_and_parentdir() {
        use super::skill_name_matches_directory;
        use std::path::Path;

        assert!(skill_name_matches_directory(
            Path::new("/tmp/wanted/SKILL.md"),
            "wanted"
        ));
        assert!(
            skill_name_matches_directory(Path::new("/tmp/wanted/./SKILL.md"), "wanted"),
            "wanted/./SKILL.md is the wanted package"
        );
        assert!(
            skill_name_matches_directory(Path::new("/tmp/wanted/other/../SKILL.md"), "wanted"),
            "wanted/other/../SKILL.md is the wanted package"
        );
        assert!(!skill_name_matches_directory(
            Path::new("/tmp/wanted/./SKILL.md"),
            "."
        ));
        assert!(!skill_name_matches_directory(
            Path::new("/tmp/other/wanted/../SKILL.md"),
            "wanted"
        ));
    }

    #[test]
    fn parse_skill_present_null_user_invocable_is_error_not_default() {
        // Same present-null rule as MCP: a typed boolean that is present
        // as YAML null / empty / garbage must not stay the omitted default.
        for (key, raw, needle) in [
            ("user_invocable", "null", "boolean"),
            ("user-invocable", "~", "boolean"),
            ("user_invocable", "", "empty"),
            ("user_invocable", "maybe", "boolean"),
            ("user_invocable", "yes\u{2028}no", "boolean"),
        ] {
            let input = format!("---\nname: demo\ndescription: d\n{key}: {raw}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                matches!(err, ParseError::InvalidYaml(_)) && msg.contains(needle),
                "present {key}: {raw:?} must not silently default true: {err}"
            );
            assert_eq!(
                msg.lines().count(),
                1,
                "parse error must stay one line: {msg:?}"
            );
            assert!(
                !msg.contains('\u{2028}'),
                "hostile bool value must be sanitized: {msg:?}"
            );
        }
        let ok = parse_skill("---\nname: demo\ndescription: d\nuser_invocable: false\n---\nbody\n")
            .expect("false must still load");
        assert!(
            !ok.user_invocable,
            "valid false must not become the omitted default"
        );
        let omitted = parse_skill("---\nname: demo\ndescription: d\n---\nbody\n").expect("omit");
        assert!(
            omitted.user_invocable,
            "omitted user_invocable still defaults true"
        );
    }

    #[test]
    fn parse_skill_present_null_disable_model_invocation_is_error_not_default() {
        for (key, raw) in [
            ("disable_model_invocation", "null"),
            ("disable-model-invocation", "~"),
            ("disable-model-invocation", ""),
            ("disable_model_invocation", "garbage"),
        ] {
            let input = format!("---\nname: demo\ndescription: d\n{key}: {raw}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            assert!(
                matches!(err, ParseError::InvalidYaml(_)),
                "present {key}: {raw:?} must not silently default false: {err}"
            );
        }
        let ok = parse_skill(
            "---\nname: demo\ndescription: d\ndisable-model-invocation: true\n---\nbody\n",
        )
        .expect("true must still load");
        assert!(ok.disable_model_invocation);
    }

    #[test]
    fn hyphen_bool_errors_use_table_snake_not_raw_yaml_key() {
        // Production table is the lock: a new hyphen/snake bool pair
        // must land here and in both parse arms. A match arm that
        // passes the raw YAML `key` into require_bool_yaml fails the
        // hyphen-not-in-message check.
        for &(hyphen, snake) in HYPHEN_BOOL_KEYS {
            assert_ne!(hyphen, snake, "alias pair must differ");
            assert!(
                hyphen.contains('-') && snake.contains('_'),
                "table is hyphen -> snake: {hyphen} / {snake}"
            );

            let scalar = format!("---\nname: demo\ndescription: d\n{hyphen}: maybe\n---\nbody\n");
            let err = parse_skill(&scalar).expect_err(&scalar).to_string();
            assert!(
                err.contains(snake) && err.contains("boolean"),
                "scalar garbage must peel {snake}: {err}"
            );
            assert!(
                !err.contains(hyphen),
                "scalar must not leak raw YAML key {hyphen}: {err}"
            );

            let empty_block = format!("---\nname: demo\ndescription: d\n{hyphen}: |\n---\nbody\n");
            let err = parse_skill(&empty_block)
                .expect_err(&empty_block)
                .to_string();
            assert!(
                err.contains(snake),
                "empty block scalar must peel {snake}: {err}"
            );
            assert!(
                !err.contains(hyphen),
                "empty block must not leak raw YAML key {hyphen}: {err}"
            );

            let garbage_block =
                format!("---\nname: demo\ndescription: d\n{hyphen}: |\n  maybe\n---\nbody\n");
            let err = parse_skill(&garbage_block)
                .expect_err(&garbage_block)
                .to_string();
            assert!(
                err.contains(snake) && err.contains("boolean"),
                "garbage block scalar must peel {snake}, not stay omitted default: {err}"
            );
            assert!(
                !err.contains(hyphen),
                "garbage block must not leak raw YAML key {hyphen}: {err}"
            );

            // Opposite of the omitted default so assignment cannot hide.
            let (raw, expect) = match snake {
                "user_invocable" => ("false", false),
                "disable_model_invocation" => ("true", true),
                other => panic!("HYPHEN_BOOL_KEYS has unassigned field {other}"),
            };
            let ok = parse_skill(&format!(
                "---\nname: demo\ndescription: d\n{hyphen}: |\n  {raw}\n---\nbody\n"
            ))
            .unwrap_or_else(|e| panic!("{hyphen} block {raw} must parse: {e}"));
            let got = match snake {
                "user_invocable" => ok.user_invocable,
                "disable_model_invocation" => ok.disable_model_invocation,
                other => panic!("HYPHEN_BOOL_KEYS has unassigned field {other}"),
            };
            assert_eq!(
                got, expect,
                "{hyphen} block {raw} must not stay the omitted default"
            );
        }

        let prod = include_str!("parse.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("prod");
        assert!(
            !prod.contains("require_bool_yaml(key"),
            "require_bool_yaml must receive the table snake name, not the raw YAML key"
        );
    }

    #[test]
    fn hyphen_bool_block_scalar_hostile_value_is_sanitized() {
        // require_bool_yaml still sanitizes U+2028 / em dash after
        // HYPHEN_BOOL_KEYS (inline scalars already cover U+2028).
        // `|` and `>` share yaml_block_scalar_style; lock both.
        for &(hyphen, snake) in HYPHEN_BOOL_KEYS {
            for style in ['|', '>'] {
                let input = format!(
                    "---\nname: demo\ndescription: d\n{hyphen}: {style}\n  yes\u{2028}no\u{2014}maybe\n---\nbody\n"
                );
                let err = parse_skill(&input).expect_err(&input);
                let msg = err.to_string();
                assert!(
                    matches!(err, ParseError::InvalidYaml(_))
                        && msg.contains(snake)
                        && msg.contains("boolean"),
                    "hostile block must peel {snake}: {msg}"
                );
                assert_eq!(
                    msg.lines().count(),
                    1,
                    "hostile block error must stay one line: {msg:?}"
                );
                assert!(
                    !msg.contains('\u{2028}'),
                    "U+2028 must not leak from block bool: {msg:?}"
                );
                assert!(
                    !msg.contains('\u{2014}'),
                    "em dash must not leak from block bool: {msg:?}"
                );
                assert!(
                    !msg.contains(hyphen),
                    "hostile {style} block must not leak raw YAML key {hyphen}: {msg}"
                );
            }
        }
    }
}
