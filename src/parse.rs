//! Hand-rolled SKILL.md frontmatter parser. Do not add `serde_yaml`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::ParseError;
use crate::skill::{
    SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS, SKILL_NAME_MAX_CHARS, Skill,
};

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

/// Validate agentskills.io `name` field rules.
pub fn validate_skill_name(name: &str) -> Result<(), ParseError> {
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
    if !name
        .chars()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
    {
        return Err(ParseError::InvalidYaml(
            "name must be lowercase alphanumeric and hyphens only".to_owned(),
        ));
    }
    Ok(())
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

/// True when the parent directory name of `skill_md` matches `name`.
pub fn skill_name_matches_directory(skill_md: &Path, name: &str) -> bool {
    skill_md
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some(name)
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
                "user-invocable" | "user_invocable" => {
                    user_invocable = parse_bool_yaml(value).unwrap_or(true);
                }
                "disable-model-invocation" | "disable_model_invocation" => {
                    disable_model_invocation = parse_bool_yaml(value).unwrap_or(false);
                }
                "argument-hint" | "argument_hint" if !value.is_empty() => {
                    argument_hint = Some(value.to_owned());
                }
                "when-to-use" | "when_to_use" if !value.is_empty() => {
                    when_to_use = Some(value.to_owned());
                }
                _ => {}
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

fn parse_bool_yaml(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
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
    use super::{parse_frontmatter, parse_skill, peek_frontmatter_name, validate_skill_name};
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
}
