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

/// Missing `name` or `description`, naming one unknown key that is a
/// one-edit, ASCII-case, or underscore miss of that field.
///
/// Two close keys stay the plain missing-field error.
fn missing_field_near_unknown_key(yaml: &str, field: &'static str) -> ParseError {
    let mut close: Option<String> = None;
    let mut ambiguous = false;
    for line in yaml.lines() {
        if line_is_yaml_indented(line) {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("- ") {
            continue;
        }
        let Some((key, _)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || is_known_frontmatter_key(key) || !frontmatter_key_is_close(key, field)
        {
            continue;
        }
        match &close {
            None => close = Some(key.to_owned()),
            Some(prev) if prev == key => {}
            Some(_) => ambiguous = true,
        }
    }
    match close {
        Some(key) if !ambiguous => {
            let shown = crate::sanitize_error_token(&key);
            ParseError::MissingField(format!(
                "{field} (unknown key: {shown}; did you mean {field}?)"
            ))
        }
        _ => ParseError::MissingField(field.to_owned()),
    }
}

fn frontmatter_key_is_close(unknown: &str, field: &str) -> bool {
    let fold = |s: &str| s.trim().to_lowercase().replace('_', "-");
    let unknown_folded = fold(unknown);
    let field_folded = fold(field);
    if unknown_folded.is_empty() || field_folded.is_empty() {
        return false;
    }
    if unknown_folded == field_folded {
        return unknown != field;
    }
    frontmatter_keys_one_edit(&unknown_folded, &field_folded)
}

/// Unicode-scalar Levenshtein distance of exactly one.
fn frontmatter_keys_one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (short, long) = if a.len() <= b.len() {
        (&a[..], &b[..])
    } else {
        (&b[..], &a[..])
    };
    let diff = long.len() - short.len();
    if diff > 1 {
        return false;
    }
    if diff == 0 {
        let mut mismatches = 0usize;
        for (x, y) in short.iter().zip(long.iter()) {
            if x != y {
                mismatches += 1;
                if mismatches > 1 {
                    return false;
                }
            }
        }
        return mismatches == 1;
    }
    let mut i = 0usize;
    let mut skipped = false;
    for ch in long {
        if i < short.len() && *ch == short[i] {
            i += 1;
        } else if !skipped {
            skipped = true;
        } else {
            return false;
        }
    }
    true
}

/// Top-level frontmatter keys that parse ignores (not known or host extensions).
pub(crate) fn unknown_frontmatter_keys(content: &str) -> Vec<String> {
    let Some(yaml) = frontmatter_yaml(content) else {
        return Vec::new();
    };
    let mut unknown = Vec::new();
    for line in yaml.lines() {
        if line_is_yaml_indented(line) {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("- ") {
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

/// YAML document-start / document-end: `---` alone on the line.
/// Trailing space and a `#` comment are allowed. `---x` and `----` are not.
fn yaml_fence_line_ok(line: &str) -> bool {
    let line = line.trim_end_matches('\r');
    if !line.starts_with("---") {
        return false;
    }
    let rest = line[3..].trim_start();
    rest.is_empty() || rest.starts_with('#')
}

/// Offset of the `\n` that precedes a valid closing `---` fence.
fn find_yaml_close(after_open: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = after_open[from..].find("\n---") {
        let nl = from + rel;
        let line_start = nl + 1;
        let line_end = after_open[line_start..]
            .find('\n')
            .map(|i| line_start + i)
            .unwrap_or(after_open.len());
        if yaml_fence_line_ok(&after_open[line_start..line_end]) {
            return Some(nl);
        }
        from = line_start;
    }
    None
}

/// Split `--- yaml --- body`. Parse, peek, and unknown-key scan share
/// this so a delimiter change cannot drift across those paths.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    // U+FEFF is not White_Space, so trim_start leaves a leading BOM.
    let trimmed = content.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let first_end = trimmed.find('\n').unwrap_or(trimmed.len());
    if !yaml_fence_line_ok(&trimmed[..first_end]) {
        return None;
    }
    // Keep the newline that ends the open fence. Stripping every leading
    // newline made `---\n---\n` look like a missing close: the close search
    // only sees a fence after `\n`.
    let after_open = if first_end < trimmed.len() {
        &trimmed[first_end + 1..]
    } else {
        ""
    };
    if fence_starts(after_open) {
        let line_end = after_open.find('\n').unwrap_or(after_open.len());
        let body = if line_end < after_open.len() {
            after_open[line_end + 1..].trim_start_matches(['\r', '\n'])
        } else {
            ""
        };
        return Some(("", body));
    }
    let close_nl = find_yaml_close(after_open)?;
    let yaml = &after_open[..close_nl];
    let after_close = &after_open[close_nl + 1..];
    let close_line_end = after_close.find('\n').unwrap_or(after_close.len());
    let body = after_close[close_line_end..].trim_start_matches(['\r', '\n']);
    Some((yaml, body))
}

fn fence_starts(text: &str) -> bool {
    let line_end = text.find('\n').unwrap_or(text.len());
    yaml_fence_line_ok(&text[..line_end])
}

fn frontmatter_yaml(content: &str) -> Option<&str> {
    split_frontmatter(content).map(|(yaml, _)| yaml)
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
    let (yaml_block, body) = split_frontmatter(content).ok_or(ParseError::MissingFrontmatter)?;
    #[cfg(test)]
    PARSE_SKILL_CONTENTS.with(|c| c.borrow_mut().push(content.to_owned()));

    let mut skill = parse_frontmatter(yaml_block)?;
    skill.content = body.to_owned();
    skill.name = normalize_skill_name(&skill.name);

    validate_skill_name(&skill.name)?;
    if skill.description.is_empty() {
        return Err(missing_field_near_unknown_key(yaml_block, "description"));
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
    let (yaml_block, _) = split_frontmatter(content)?;
    if let Ok(skill) = parse_frontmatter(yaml_block) {
        return Some(skill.name);
    }
    scan_frontmatter_name(yaml_block)
}

#[cfg(test)]
thread_local! {
    static PARSE_SKILL_CONTENTS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Drain [`parse_skill`] bodies recorded in this test thread.
#[cfg(test)]
pub(crate) fn take_parse_skill_contents() -> Vec<String> {
    PARSE_SKILL_CONTENTS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

fn scan_frontmatter_name(yaml: &str) -> Option<String> {
    for line in yaml.lines() {
        if line_is_yaml_indented(line) {
            continue;
        }
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
        let value = unquote_yaml_scalar(raw_value);
        if value.is_empty() {
            return None;
        }
        return Some(value);
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

/// Space, tab, or other White_Space. `trim()` would peel these, so they
/// are not top-level keys.
fn line_is_yaml_indented(line: &str) -> bool {
    matches!(line.chars().next(), Some(c) if c.is_whitespace())
}

fn yaml_indent_width(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

fn peek_starts_yaml_list(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) -> bool {
    loop {
        let skip = match lines.peek().copied() {
            Some(line) => {
                let trimmed = line.trim();
                trimmed.is_empty() || trimmed.starts_with('#')
            }
            None => return false,
        };
        if !skip {
            break;
        }
        lines.next();
    }
    lines
        .peek()
        .copied()
        .is_some_and(|line| line.trim().starts_with("- "))
}

/// Unquoted YAML null (`null`, `Null`, `NULL`, `~`).
/// A quoted `"null"` is the word, not an empty scalar.
fn unquoted_yaml_null(raw: &str) -> bool {
    matches!(raw.trim(), "null" | "Null" | "NULL" | "~")
}

/// Unquoted YAML 1.1 boolean words (`true`, `yes`, `on`, and the
/// false set). Case folds. Digits `0` and `1` stay text because
/// metadata `version: 1` is a string. A quoted `"true"` is the word.
fn unquoted_yaml_bool_word(raw: &str) -> bool {
    if quoted_yaml_scalar(raw) {
        return false;
    }
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off"
    )
}

/// A scalar wrapped in `"` or `'`. The whole value is one token.
fn quoted_yaml_scalar(raw: &str) -> bool {
    let s = raw.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    let quote = bytes[0];
    (quote == b'"' || quote == b'\'') && bytes[bytes.len() - 1] == quote
}

/// Unquoted flow sequence or flow map. Quoted text may contain brackets.
fn unquoted_yaml_flow_collection(raw: &str) -> bool {
    let s = raw.trim();
    if s.starts_with('"') || s.starts_with('\'') {
        return false;
    }
    s.starts_with('[') || s.starts_with('{')
}

fn reject_optional_string_flow(key: &str, raw_value: &str) -> Result<(), ParseError> {
    if !unquoted_yaml_flow_collection(raw_value) {
        return Ok(());
    }
    let shown_key = crate::sanitize_error_token(key);
    let shown = crate::sanitize_error_token(raw_value.trim());
    Err(ParseError::InvalidYaml(format!(
        "{shown_key} must be a string, got: {shown}"
    )))
}

/// Unquoted null in a metadata pair is omitted. Quoted `"null"` stays.
fn metadata_scalar_or_skip(raw: &str) -> Option<String> {
    if unquoted_yaml_null(raw) {
        None
    } else {
        Some(unquote_yaml_scalar(raw))
    }
}

/// `argument-hint: [name]` is placeholder text. A flow map is not.
fn unquoted_argument_hint_brackets(raw_value: &str) -> bool {
    let s = raw_value.trim();
    if s.starts_with('"') || s.starts_with('\'') {
        return false;
    }
    s.starts_with('[')
}

fn reject_optional_string_bool(key: &str, raw_value: &str) -> Result<(), ParseError> {
    if !unquoted_yaml_bool_word(raw_value) {
        return Ok(());
    }
    let shown_key = crate::sanitize_error_token(key);
    let shown = crate::sanitize_error_token(raw_value.trim());
    Err(ParseError::InvalidYaml(format!(
        "{shown_key} must be a string, got: {shown}"
    )))
}

fn reject_metadata_bool_word(raw: &str) -> Result<(), ParseError> {
    if !unquoted_yaml_bool_word(raw) {
        return Ok(());
    }
    let shown = crate::sanitize_error_token(raw.trim());
    Err(ParseError::InvalidYaml(format!(
        "metadata value must be a string, got: {shown}"
    )))
}

fn unquoted_yaml_flow_map(raw: &str) -> bool {
    unquoted_yaml_flow_collection(raw) && raw.trim().starts_with('{')
}

/// Next significant line when it is an indented `key:` (a nested map).
/// A following `- item` stays in the iterator so the list error still
/// names that item. Blank lines and comments are skipped.
fn peek_indented_nested_key<'a, I>(lines: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = &'a str>,
{
    loop {
        let skip = match lines.peek().copied() {
            Some(line) => {
                let trimmed = line.trim();
                trimmed.is_empty() || trimmed.starts_with('#')
            }
            None => return None,
        };
        if !skip {
            break;
        }
        lines.next();
    }
    let line = lines.peek().copied()?;
    if !line_is_yaml_indented(line) {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.starts_with("- ") || trimmed == "-" {
        return None;
    }
    if trimmed.split_once(':').is_some() {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

/// A plain fold stops at a mapping entry. Dropping that line would
/// keep a truncated value and still validate. A `{...}` / `[...]`
/// flow value stays on the indent-skip path: it is not folded, and
/// it does not fail the skill.
fn reject_nested_key_after_value<'a, I>(
    key: &str,
    kind: &str,
    lines: &mut std::iter::Peekable<I>,
) -> Result<(), ParseError>
where
    I: Iterator<Item = &'a str>,
{
    let Some(nested) = peek_indented_nested_key(lines) else {
        return Ok(());
    };
    if unquoted_yaml_flow_collection(&nested) {
        return Ok(());
    }
    let shown_key = crate::sanitize_error_token(key);
    let shown = crate::sanitize_error_token(&nested);
    Err(ParseError::InvalidYaml(format!(
        "{shown_key} must be {kind}, got: {shown}"
    )))
}

fn optional_frontmatter_string<'a, I>(
    key: &str,
    field_text: &str,
    lines: &mut std::iter::Peekable<I>,
    allow_argument_hint_brackets: bool,
) -> Result<Option<String>, ParseError>
where
    I: Iterator<Item = &'a str> + Clone,
{
    let raw_value = strip_yaml_inline_comment(field_text);
    let value = unquote_yaml_scalar(raw_value);
    if unquoted_yaml_null(raw_value) {
        if let Some(nested) = peek_indented_nested_key(lines) {
            let shown_key = crate::sanitize_error_token(key);
            let shown = crate::sanitize_error_token(&nested);
            return Err(ParseError::InvalidYaml(format!(
                "{shown_key} must be a string, got: {shown}"
            )));
        }
        return Ok(None);
    }
    if value.is_empty() {
        if quoted_yaml_scalar(raw_value) {
            if let Some(nested) = peek_indented_nested_key(lines) {
                let shown_key = crate::sanitize_error_token(key);
                let shown = crate::sanitize_error_token(&nested);
                return Err(ParseError::InvalidYaml(format!(
                    "{shown_key} must be a string, got: {shown}"
                )));
            }
            return Ok(None);
        }
        return match read_empty_scalar_follow(key, lines)? {
            EmptyScalarFollow::Value(text) => Ok(Some(text)),
            EmptyScalarFollow::List | EmptyScalarFollow::Absent => Ok(None),
            EmptyScalarFollow::Mapping(shown) | EmptyScalarFollow::Flow(shown) => {
                let shown_key = crate::sanitize_error_token(key);
                let shown = crate::sanitize_error_token(&shown);
                Err(ParseError::InvalidYaml(format!(
                    "{shown_key} must be a string, got: {shown}"
                )))
            }
        };
    }
    if !(allow_argument_hint_brackets && unquoted_argument_hint_brackets(raw_value)) {
        reject_optional_string_flow(key, raw_value)?;
    }
    reject_optional_string_bool(key, raw_value)?;
    let stored = extend_plain_scalar(&value, field_text, lines);
    reject_nested_key_after_value(key, "a string", lines)?;
    Ok(Some(stored))
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
    let mut in_ignore_sequence = false;
    let mut in_metadata = false;
    let mut metadata_key_indent: Option<usize> = None;

    let mut lines = yaml.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if in_metadata {
            let is_indented = line_is_yaml_indented(line);
            if is_indented {
                let indent = yaml_indent_width(line);
                if metadata_key_indent.is_some_and(|base| indent > base) {
                    let shown = crate::sanitize_error_token(trimmed);
                    return Err(ParseError::InvalidYaml(format!(
                        "metadata value must be a string, got: {shown}"
                    )));
                }
                if metadata_key_indent.is_none() {
                    metadata_key_indent = Some(indent);
                }
                if trimmed.starts_with("- ") || trimmed == "-" {
                    let shown = crate::sanitize_error_token(trimmed);
                    return Err(ParseError::InvalidYaml(format!(
                        "metadata value must be a string, got: {shown}"
                    )));
                }
                if let Some((k, v)) = trimmed.split_once(':') {
                    let k = unquote_yaml_scalar(k.trim());
                    let raw_v = strip_yaml_inline_comment(v);
                    if unquoted_yaml_flow_collection(raw_v) {
                        let shown = crate::sanitize_error_token(raw_v.trim());
                        return Err(ParseError::InvalidYaml(format!(
                            "metadata value must be a string, got: {shown}"
                        )));
                    }
                    if raw_v.trim().is_empty() {
                        let shown = crate::sanitize_error_token(&k);
                        return Err(ParseError::InvalidYaml(format!(
                            "metadata {shown} must be a string"
                        )));
                    }
                    reject_metadata_bool_word(raw_v)?;
                    if let Some(stored) = metadata_scalar_or_skip(raw_v) {
                        if !k.is_empty() {
                            metadata.insert(k, stored);
                        }
                    }
                } else {
                    let shown = crate::sanitize_error_token(trimmed);
                    return Err(ParseError::InvalidYaml(format!(
                        "metadata value must be a string, got: {shown}"
                    )));
                }
                continue;
            }
            in_metadata = false;
            metadata_key_indent = None;
        }

        if trimmed.starts_with("- ") && in_triggers {
            let item = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            push_wrapped_trigger_item(&mut triggers, item, &mut lines);
            continue;
        }
        if trimmed.starts_with("- ") && in_ignore_sequence {
            continue;
        }

        in_triggers = false;
        in_ignore_sequence = false;

        // A list item can contain a colon (`- use foo: bar`). That is
        // not a nested key. Name the item. Do not fall through to a
        // missing description.
        if trimmed.starts_with("- ") {
            let shown = crate::sanitize_error_token(trimmed);
            return Err(ParseError::InvalidYaml(format!(
                "expected `key: value`, got: {shown}"
            )));
        }

        if line_is_yaml_indented(line) && trimmed.split_once(':').is_some() {
            continue;
        }

        if let Some((key, field_text)) = trimmed.split_once(':') {
            let key = key.trim();
            let raw_value = strip_yaml_inline_comment(field_text);
            let value = unquote_yaml_scalar(raw_value);

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
                    let shown = crate::sanitize_error_token(key);
                    return Err(ParseError::InvalidYaml(format!(
                        "{shown} block scalar is empty"
                    )));
                }
                match key {
                    "description" => description = Some(block),
                    "license" => license = Some(block),
                    "compatibility" => compatibility = Some(block),
                    "allowed-tools" | "allowed_tools" => allowed_tools = Some(block),
                    "argument-hint" | "argument_hint" => argument_hint = Some(block),
                    "when-to-use" | "when_to_use" => when_to_use = Some(block),
                    "triggers" => push_trigger_block_lines(&mut triggers, &block),
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
                    if value.is_empty()
                        || unquoted_yaml_null(raw_value)
                        || unquoted_yaml_bool_word(raw_value)
                    {
                        if !peek_starts_yaml_list(&mut lines) {
                            return Err(ParseError::InvalidYaml("name value is empty".to_owned()));
                        }
                    } else {
                        reject_nested_key_after_value("name", "a string", &mut lines)?;
                        name = Some(value);
                    }
                }
                "description" => {
                    if unquoted_yaml_flow_collection(raw_value) {
                        let shown = crate::sanitize_error_token(raw_value.trim());
                        return Err(ParseError::InvalidYaml(format!(
                            "description must be a string, got: {shown}"
                        )));
                    }
                    if unquoted_yaml_null(raw_value) || unquoted_yaml_bool_word(raw_value) {
                        if !peek_starts_yaml_list(&mut lines) {
                            return Err(ParseError::InvalidYaml(
                                "description value is empty".to_owned(),
                            ));
                        }
                    } else if value.is_empty() {
                        if quoted_yaml_scalar(raw_value) {
                            if !peek_starts_yaml_list(&mut lines) {
                                return Err(ParseError::InvalidYaml(
                                    "description value is empty".to_owned(),
                                ));
                            }
                        } else {
                            match read_empty_scalar_follow("description", &mut lines)? {
                                EmptyScalarFollow::Value(text) => description = Some(text),
                                EmptyScalarFollow::Flow(shown) => {
                                    let shown = crate::sanitize_error_token(&shown);
                                    return Err(ParseError::InvalidYaml(format!(
                                        "description must be a string, got: {shown}"
                                    )));
                                }
                                EmptyScalarFollow::List => {}
                                EmptyScalarFollow::Absent | EmptyScalarFollow::Mapping(_) => {
                                    return Err(ParseError::InvalidYaml(
                                        "description value is empty".to_owned(),
                                    ));
                                }
                            }
                        }
                    } else {
                        let stored = extend_plain_scalar(&value, field_text, &mut lines);
                        reject_nested_key_after_value("description", "a string", &mut lines)?;
                        description = Some(stored);
                    }
                }
                "triggers" => {
                    if value.is_empty() {
                        in_triggers = true;
                    } else if !unquoted_yaml_null(raw_value) {
                        if quoted_yaml_scalar(raw_value) {
                            reject_nested_key_after_value("triggers", "a string", &mut lines)?;
                            triggers.push(value);
                        } else {
                            let folded = extend_plain_scalar(&value, field_text, &mut lines);
                            reject_nested_key_after_value("triggers", "a string", &mut lines)?;
                            push_inline_triggers(&mut triggers, &folded)?;
                        }
                    }
                }
                "license" => {
                    if let Some(stored) =
                        optional_frontmatter_string(key, field_text, &mut lines, false)?
                    {
                        license = Some(stored);
                    }
                }
                "compatibility" => {
                    if let Some(stored) =
                        optional_frontmatter_string(key, field_text, &mut lines, false)?
                    {
                        compatibility = Some(stored);
                    }
                }
                "allowed-tools" | "allowed_tools" => {
                    if let Some(stored) =
                        optional_frontmatter_string(key, field_text, &mut lines, false)?
                    {
                        allowed_tools = Some(stored);
                    }
                }
                "metadata" if !line_is_yaml_indented(line) => {
                    if value.is_empty() {
                        in_metadata = true;
                    } else {
                        push_inline_metadata(&mut metadata, &value)?;
                        reject_nested_key_after_value("metadata value", "a string", &mut lines)?;
                    }
                }
                "argument-hint" | "argument_hint" => {
                    if let Some(stored) =
                        optional_frontmatter_string(key, field_text, &mut lines, true)?
                    {
                        argument_hint = Some(stored);
                    }
                }
                "when-to-use" | "when_to_use" => {
                    if let Some(stored) =
                        optional_frontmatter_string(key, field_text, &mut lines, false)?
                    {
                        when_to_use = Some(stored);
                    }
                }
                _ => {
                    if let Some(canon) = canonical_bool_yaml_key(key) {
                        // A block list is not an empty boolean. The `- `
                        // check names the item.
                        let list_follows = value.is_empty() && peek_starts_yaml_list(&mut lines);
                        if !list_follows {
                            assign_parsed_bool(
                                canon,
                                require_bool_yaml(canon, &value)?,
                                &mut user_invocable,
                                &mut disable_model_invocation,
                            );
                            reject_nested_key_after_value(canon, "a boolean", &mut lines)?;
                        }
                    } else if key == "author" && value.is_empty() {
                        if peek_indented_nested_key(&mut lines).is_some() {
                            return Err(ParseError::InvalidYaml(
                                "metadata author must be a string".to_owned(),
                            ));
                        }
                        in_ignore_sequence = true;
                    } else if key == "author" && unquoted_yaml_flow_map(raw_value) {
                        let shown = crate::sanitize_error_token(raw_value.trim());
                        return Err(ParseError::InvalidYaml(format!(
                            "metadata value must be a string, got: {shown}"
                        )));
                    } else if value.is_empty() && !is_known_frontmatter_key(key) {
                        // Unknown key with a block sequence: ignore items.
                        // Known scalars (`license:`) stay empty and a
                        // following `- item` is still InvalidYaml.
                        in_ignore_sequence = true;
                    }
                }
            }
        } else {
            let shown = crate::sanitize_error_token(trimmed);
            return Err(ParseError::InvalidYaml(format!(
                "expected `key: value`, got: {shown}"
            )));
        }
    }

    let name = name.ok_or_else(|| missing_field_near_unknown_key(yaml, "name"))?;
    let description =
        description.ok_or_else(|| missing_field_near_unknown_key(yaml, "description"))?;

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
        if !line_is_yaml_indented(next) {
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

enum EmptyScalarFollow {
    Absent,
    List,
    Mapping(String),
    Flow(String),
    Value(String),
}

struct FoldedPlain {
    text: String,
    broke_before: bool,
}

/// `key:` with nothing on the same line. A following indented plain
/// line or block scalar is the value. A list item stays in the
/// iterator so the list error can name it. A nested `key:` stays too.
fn read_empty_scalar_follow<'a, I>(
    key: &str,
    lines: &mut std::iter::Peekable<I>,
) -> Result<EmptyScalarFollow, ParseError>
where
    I: Iterator<Item = &'a str> + Clone,
{
    loop {
        let Some(line) = lines.peek().copied() else {
            return Ok(EmptyScalarFollow::Absent);
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.next();
            continue;
        }
        if !line_is_yaml_indented(line) {
            return Ok(EmptyScalarFollow::Absent);
        }
        if trimmed.starts_with("- ") || trimmed == "-" {
            return Ok(EmptyScalarFollow::List);
        }
        let stripped = strip_yaml_inline_comment(trimmed);
        if unquoted_yaml_flow_collection(stripped) {
            return Ok(EmptyScalarFollow::Flow(stripped.to_owned()));
        }
        if let Some(style) = yaml_block_scalar_style(stripped) {
            lines.next();
            let block = take_yaml_block_scalar(lines, style);
            if block.is_empty() {
                let shown_key = crate::sanitize_error_token(key);
                return Err(ParseError::InvalidYaml(format!(
                    "{shown_key} block scalar is empty"
                )));
            }
            return Ok(EmptyScalarFollow::Value(block));
        }
        if plain_line_is_mapping_entry(stripped) {
            return Ok(EmptyScalarFollow::Mapping(stripped.to_owned()));
        }
        let Some(folded) = pull_plain_continuations(lines) else {
            return Ok(EmptyScalarFollow::Absent);
        };
        return Ok(EmptyScalarFollow::Value(folded.text));
    }
}

/// Fold indented plain lines into an unquoted scalar. Quoted text,
/// an inline comment, and `|` / `>` heads stay a single token.
fn extend_plain_scalar<'a, I>(
    head: &str,
    field_text: &str,
    lines: &mut std::iter::Peekable<I>,
) -> String
where
    I: Iterator<Item = &'a str> + Clone,
{
    if !plain_scalar_head_continues(field_text) {
        return head.to_owned();
    }
    match pull_plain_continuations(lines) {
        Some(folded) => join_plain_head(head, &folded),
        None => head.to_owned(),
    }
}

fn plain_scalar_head_continues(field_text: &str) -> bool {
    if yaml_field_has_inline_comment(field_text) {
        return false;
    }
    let raw_value = strip_yaml_inline_comment(field_text);
    let trimmed = raw_value.trim();
    if trimmed.is_empty()
        || quoted_yaml_scalar(raw_value)
        || unquoted_yaml_null(raw_value)
        || unquoted_yaml_bool_word(raw_value)
        || unquoted_yaml_flow_collection(raw_value)
    {
        return false;
    }
    !matches!(trimmed.chars().next(), Some('|' | '>'))
}

fn yaml_field_has_inline_comment(field_text: &str) -> bool {
    let trimmed = field_text.trim();
    strip_yaml_inline_comment(field_text).len() != trimmed.len()
}

/// A mapping entry is `key:` or `key:` plus space or tab. `foo:bar`
/// and `https://example.com/a:b` stay plain text. Quotes hide a colon.
fn plain_line_is_mapping_entry(trimmed: &str) -> bool {
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b':' => {
                let next = bytes.get(i + 1).copied();
                if next.is_none() || next == Some(b' ') || next == Some(b'\t') {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

fn is_plain_continuation_line(line: &str) -> bool {
    if !line_is_yaml_indented(line) {
        return false;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }
    if trimmed.starts_with("- ") || trimmed == "-" {
        return false;
    }
    let stripped = strip_yaml_inline_comment(trimmed);
    if yaml_block_scalar_style(stripped).is_some() || plain_line_is_mapping_entry(stripped) {
        return false;
    }
    true
}

fn pull_plain_continuations<'a, I>(lines: &mut std::iter::Peekable<I>) -> Option<FoldedPlain>
where
    I: Iterator<Item = &'a str> + Clone,
{
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut broke_before = false;
    let mut seen = false;
    loop {
        let Some(peeked) = lines.peek().copied() else {
            break;
        };
        if peeked.trim().is_empty() {
            if !blank_then_plain_continuation(lines) {
                break;
            }
            lines.next();
            if seen && !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            } else if !seen {
                broke_before = true;
            }
            continue;
        }
        if peeked.trim().starts_with('#') {
            lines.next();
            continue;
        }
        if !is_plain_continuation_line(peeked) {
            break;
        }
        let Some(raw) = lines.next() else {
            break;
        };
        let piece = unquote_yaml_scalar(strip_yaml_inline_comment(raw));
        if !piece.is_empty() {
            current.push(piece);
            seen = true;
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    if paragraphs.is_empty() {
        None
    } else {
        Some(FoldedPlain {
            text: paragraphs.join("\n"),
            broke_before,
        })
    }
}

fn blank_then_plain_continuation<'a, I>(lines: &std::iter::Peekable<I>) -> bool
where
    I: Iterator<Item = &'a str> + Clone,
{
    let mut look = lines.clone();
    look.next();
    loop {
        let Some(line) = look.peek().copied() else {
            return false;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            look.next();
            continue;
        }
        return is_plain_continuation_line(line);
    }
}

fn join_plain_head(head: &str, folded: &FoldedPlain) -> String {
    if folded.broke_before {
        format!("{head}\n{}", folded.text)
    } else {
        format!("{head} {}", folded.text)
    }
}

/// Split a flow-map or flow-list interior on commas outside quotes.
///
/// `"Doe, Jane"` and `'Doe, Jane'` are one item. An unclosed quote
/// consumes the rest, so a comma after that quote does not split.
fn split_yaml_flow_items(inner: &str) -> Vec<&str> {
    let bytes = inner.as_bytes();
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                i += quoted_scalar_len(&inner[i..]);
            }
            b',' => {
                items.push(inner[start..i].trim());
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    items.push(inner[start..].trim());
    items
}

/// Byte length of a quoted scalar starting at `s`, including the opener.
/// Unclosed quotes consume all of `s`.
fn quoted_scalar_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let q = bytes[0];
    if q != b'"' && q != b'\'' {
        return 1;
    }
    let mut i = 1usize;
    while i < bytes.len() {
        if q == b'"' && bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if q == b'\'' && bytes[i] == b'\'' && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
            i += 2;
            continue;
        }
        if bytes[i] == q {
            return i + 1;
        }
        i += 1;
    }
    s.len()
}

/// Official agentskills `metadata` as a flow map (`{k: v, k2: v2}`).
/// A present scalar is InvalidYaml, not a silent empty map.
fn push_inline_metadata(
    metadata: &mut BTreeMap<String, String>,
    raw: &str,
) -> Result<(), ParseError> {
    let trimmed = raw.trim();
    let inner = match trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(inner) => inner,
        None => {
            let shown = crate::sanitize_error_token(trimmed);
            return Err(ParseError::InvalidYaml(format!(
                "metadata must be a map, got: {shown}"
            )));
        }
    };
    for part in split_yaml_flow_items(inner) {
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once(':') else {
            let shown = crate::sanitize_error_token(part);
            return Err(ParseError::InvalidYaml(format!(
                "metadata pair must be `key: value`, got: {shown}"
            )));
        };
        let k_raw = k.trim();
        if !flow_item_is_one_scalar(k_raw) {
            let shown = crate::sanitize_error_token(part);
            return Err(ParseError::InvalidYaml(format!(
                "metadata pair must be `key: value`, got: {shown}"
            )));
        }
        let k = unquote_yaml_scalar(k_raw);
        let raw_v = v.trim();
        if !flow_item_is_one_scalar(raw_v) {
            let shown = crate::sanitize_error_token(raw_v);
            return Err(ParseError::InvalidYaml(format!(
                "metadata value must be a string, got: {shown}"
            )));
        }
        if unquoted_yaml_flow_collection(raw_v) {
            let shown = crate::sanitize_error_token(raw_v);
            return Err(ParseError::InvalidYaml(format!(
                "metadata value must be a string, got: {shown}"
            )));
        }
        reject_metadata_bool_word(raw_v)?;
        if let Some(stored) = metadata_scalar_or_skip(raw_v) {
            if !k.is_empty() {
                metadata.insert(k, stored);
            }
        }
    }
    Ok(())
}

/// One trigger per non-empty line. `>` has already joined a paragraph
/// into one phrase, so a folded block stays a single trigger.
fn push_trigger_block_lines(triggers: &mut Vec<String>, block: &str) {
    for line in block.lines() {
        let item = line.trim();
        if !item.is_empty() {
            triggers.push(item.to_owned());
        }
    }
}

/// A quoted list item is finished. A plain item folds the following
/// indented lines into one phrase. Triggers are not paragraphs, so a
/// blank line inside the item still joins with a space.
fn push_wrapped_trigger_item<'a, I>(
    triggers: &mut Vec<String>,
    item: &str,
    lines: &mut std::iter::Peekable<I>,
) where
    I: Iterator<Item = &'a str> + Clone,
{
    let raw_item = strip_yaml_inline_comment(item);
    let mut text = unquote_yaml_scalar(raw_item);
    if text.is_empty() {
        return;
    }
    if quoted_yaml_scalar(raw_item) || yaml_block_scalar_style(raw_item.trim()).is_some() {
        triggers.push(text);
        return;
    }
    if let Some(folded) = pull_plain_continuations(lines) {
        if !folded.text.is_empty() {
            text.push(' ');
            text.push_str(&folded.text.replace('\n', " "));
        }
    }
    triggers.push(text);
}

fn push_inline_triggers(triggers: &mut Vec<String>, raw: &str) -> Result<(), ParseError> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    for raw_part in split_yaml_flow_items(inner) {
        if unquoted_yaml_null(raw_part) {
            continue;
        }
        if !flow_item_is_one_scalar(raw_part) {
            let shown = crate::sanitize_error_token(raw_part);
            return Err(ParseError::InvalidYaml(format!(
                "trigger must be one scalar, got: {shown}"
            )));
        }
        let item = unquote_yaml_scalar(raw_part);
        if !item.is_empty() {
            triggers.push(item.to_owned());
        }
    }
    Ok(())
}

/// One flow item is one scalar. A quoted scalar must consume the item.
///
/// `"Doe, Jane" version: "1"` is two scalars and no comma. An unclosed
/// quote still consumes the rest, matching [`quoted_scalar_len`].
fn flow_item_is_one_scalar(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() {
        return true;
    }
    let bytes = s.as_bytes();
    if bytes[0] != b'"' && bytes[0] != b'\'' {
        return true;
    }
    quoted_scalar_len(s) == s.len()
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
            if q == b'\'' && rest[i] == b'\'' && i + 1 < rest.len() && rest[i + 1] == b'\'' {
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

/// Strip one matching quote pair and decode YAML escapes. Unquoted
/// scalars keep leading or trailing `"` / `'`.
fn unquote_yaml_scalar(raw: &str) -> String {
    let s = raw.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        return s[1..s.len() - 1].replace("''", "'");
    }
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        return unescape_double_quoted(&s[1..s.len() - 1]);
    }
    s.to_owned()
}

fn unescape_double_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        HYPHEN_BOOL_KEYS, is_known_frontmatter_key, parse_frontmatter, parse_skill,
        peek_frontmatter_name, skill_name_matches_directory, split_frontmatter,
        unknown_frontmatter_keys, validate_skill_name,
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
    fn missing_description_names_a_one_edit_key() {
        let input = "\
---
name: demo
descripton: hello from the skill
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required field: description (unknown key: descripton; did you mean description?)"
        );
    }

    #[test]
    fn missing_description_names_an_underscore_key() {
        let input = "\
---
name: demo
descript_on: hello from the skill
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required field: description (unknown key: descript_on; did you mean description?)"
        );
    }

    #[test]
    fn missing_description_names_an_ascii_case_key() {
        let input = "\
---
name: demo
Description: hello from the skill
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required field: description (unknown key: Description; did you mean description?)"
        );
    }

    #[test]
    fn missing_description_ignores_an_unrelated_unknown_key() {
        let input = "\
---
name: demo
made_up_field: x
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(ref f) if f == "description"));
    }

    #[test]
    fn missing_description_two_close_keys_stay_plain() {
        let input = "\
---
name: demo
descripton: one
descriptio: two
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert!(matches!(err, ParseError::MissingField(ref f) if f == "description"));
    }

    #[test]
    fn missing_name_names_a_one_edit_key() {
        let input = "\
---
nme: demo
description: hello
---
body
";
        let err = parse_skill(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required field: name (unknown key: nme; did you mean name?)"
        );
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
    fn plain_wrapped_description_folds_and_keeps_the_body() {
        let input = "\
---
name: api
description: REST API conventions for this codebase, route structure,
  error handling, and auth checks before adding a new endpoint.
triggers:
  - api
---
# Body
Keep the body.
";
        let skill = parse_skill(input).expect("wrapped description should parse");
        assert_eq!(
            skill.description,
            "REST API conventions for this codebase, route structure, error handling, and auth checks before adding a new endpoint."
        );
        assert_eq!(skill.triggers, vec!["api".to_owned()]);
        assert!(skill.content.contains("Keep the body."));
        assert_eq!(skill.name, "api");
    }

    #[test]
    fn next_line_frontmatter_strings_are_the_value() {
        let description = parse_skill(
            "\
---
name: api
description:
  A long description on the next line.
---
body
",
        )
        .expect("next-line description");
        assert_eq!(
            description.description,
            "A long description on the next line."
        );

        let input = "\
---
name: api
description: d
license:
  Apache-2.0
  WITH LLVM-exception
compatibility:
  Requires git.
allowed-tools:
  Bash Read
when-to-use:
  Reviewing a pull request
argument-hint:
  file path
---
body
";
        let skill = parse_skill(input).expect("next-line optional strings");
        assert_eq!(
            skill.license.as_deref(),
            Some("Apache-2.0 WITH LLVM-exception")
        );
        assert_eq!(skill.compatibility.as_deref(), Some("Requires git."));
        assert_eq!(skill.allowed_tools.as_deref(), Some("Bash Read"));
        assert_eq!(
            skill.when_to_use.as_deref(),
            Some("Reviewing a pull request")
        );
        assert_eq!(skill.argument_hint.as_deref(), Some("file path"));
    }

    #[test]
    fn plain_description_blank_line_starts_a_paragraph() {
        let skill = parse_skill(
            "\
---
name: api
description: hello

  more text
---
body
",
        )
        .expect("paragraph break");
        assert_eq!(skill.description, "hello\nmore text");

        let stopped = parse_skill(
            "\
---
description: hello

name: api
---
body
",
        )
        .expect("blank line does not eat the next key");
        assert_eq!(stopped.description, "hello");
        assert_eq!(stopped.name, "api");

        let later = parse_skill(
            "\
---
name: api
description: hello
  line two

  line three
---
body
",
        )
        .expect("later paragraph");
        assert_eq!(later.description, "hello line two\nline three");

        let noted = parse_skill(
            "\
---
name: api
description: hello
  # note
  world # comment
---
body
",
        )
        .expect("comments inside a continuation");
        assert_eq!(noted.description, "hello world");
    }

    #[test]
    fn plain_continuation_keeps_a_colon_that_is_not_a_key() {
        let skill = parse_skill(
            "\
---
name: api
description: See the docs at
  https://example.com/a:b and foo:bar today.
---
body
",
        )
        .expect("colon inside a continuation is text");
        assert_eq!(
            skill.description,
            "See the docs at https://example.com/a:b and foo:bar today."
        );

        let flow = parse_skill(
            "\
---
name: api
description: hello
  [a, b]
---
body
",
        )
        .expect("indented flow text after a scalar");
        assert_eq!(flow.description, "hello [a, b]");

        let mapping = parse_skill(
            "\
---
name: api
description: hello
  {a: b}
---
body
",
        )
        .expect("indented mapping is not folded into the description");
        assert_eq!(mapping.description, "hello");
    }

    #[test]
    fn next_line_block_scalar_matches_the_same_line_form() {
        let literal = parse_skill(
            "\
---
name: api
description:
  |
  line one
  line two
---
body
",
        )
        .expect("next-line literal");
        assert_eq!(literal.description, "line one\nline two");

        let folded = parse_skill(
            "\
---
name: api
description: d
compatibility:
  >
  Requires
  git.
---
body
",
        )
        .expect("next-line folded compatibility");
        assert_eq!(folded.compatibility.as_deref(), Some("Requires git."));
    }

    #[test]
    fn quoted_or_commented_description_does_not_swallow_the_next_line() {
        let quoted = "\
---
name: api
description: \"hello\"
  more text
---
body
";
        let err = parse_skill(quoted).expect_err("quoted head");
        let msg = err.to_string();
        assert!(
            msg.contains("expected `key: value`") && msg.contains("more text"),
            "{msg}"
        );

        let commented = "\
---
name: api
description: hello # c
  more text
---
body
";
        let err = parse_skill(commented).expect_err("commented head");
        let msg = err.to_string();
        assert!(
            msg.contains("expected `key: value`") && msg.contains("more text"),
            "{msg}"
        );
    }

    #[test]
    fn empty_description_flow_on_the_next_line_is_not_a_string() {
        let err = parse_skill(
            "\
---
name: api
description:
  [a, b]
---
body
",
        )
        .expect_err("flow sequence");
        assert!(
            err.to_string()
                .contains("description must be a string, got: [a, b]"),
            "{err}"
        );
        let err = parse_skill(
            "\
---
name: api
description: d
license:
  {spdx: MIT}
---
body
",
        )
        .expect_err("flow map");
        assert!(
            err.to_string()
                .contains("license must be a string, got: {spdx: MIT}"),
            "{err}"
        );
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
    fn wrapped_trigger_list_item_is_one_phrase() {
        let input = "\
---
name: trigwrap
description: wrapped trigger item
triggers:
  - review a pull request that changes the public API
    and needs a changelog entry
  - code review
---
body
";
        let skill = parse_skill(input).expect("wrapped trigger item");
        assert_eq!(
            skill.triggers,
            vec![
                "review a pull request that changes the public API and needs a changelog entry"
                    .to_owned(),
                "code review".to_owned(),
            ]
        );
    }

    #[test]
    fn wrapped_inline_triggers_keep_the_next_line() {
        let input = "\
---
name: triginline
description: inline triggers wrapped
triggers: pull request,
  code review
---
body
";
        let skill = parse_skill(input).expect("wrapped inline triggers");
        assert_eq!(
            skill.triggers,
            vec!["pull request".to_owned(), "code review".to_owned()]
        );
    }

    #[test]
    fn trigger_block_scalar_keeps_each_line() {
        let literal = "\
---
name: trigblock
description: trigger block scalar
triggers: |
  pull request
  name: not-a-key
  code review
---
body
";
        let skill = parse_skill(literal).expect("literal triggers");
        assert_eq!(skill.name, "trigblock");
        assert_eq!(
            skill.triggers,
            vec![
                "pull request".to_owned(),
                "name: not-a-key".to_owned(),
                "code review".to_owned(),
            ]
        );

        let folded = "\
---
name: trigfold
description: folded triggers
triggers: >
  pull request
  code review
---
body
";
        let skill = parse_skill(folded).expect("folded triggers");
        assert_eq!(skill.triggers, vec!["pull request code review".to_owned()]);
    }

    #[test]
    fn quoted_trigger_item_does_not_swallow_the_next_line() {
        let input = "\
---
name: trigquote
description: quoted trigger stays closed
triggers:
  - \"pull request\"
    extra
---
body
";
        let err = parse_skill(input).expect_err("quoted item is finished");
        assert!(
            err.to_string().contains("extra"),
            "next line must stay an error: {err}"
        );
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
    fn description_block_list_is_not_an_empty_value() {
        let input = "\
---
name: desc-list
description:
  - not
  - a
  - string
---
# X
";
        let err = parse_skill(input).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("- not"),
            "list item must be in the error, got {msg}"
        );
        assert!(
            !msg.contains("value is empty"),
            "a list is not an empty description: {msg}"
        );
        let empty = "---\nname: desc-empty\ndescription:\n---\n# X\n";
        let empty_err = parse_skill(empty).unwrap_err();
        assert!(
            empty_err.to_string().contains("description value is empty"),
            "{empty_err}"
        );
    }

    #[test]
    fn description_block_list_item_with_colon_names_the_item() {
        let input = "\
---
name: desc-colon
description:
  - use foo: bar
---
# X
";
        let err = parse_skill(input).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("- use foo: bar"),
            "list item must be in the error, got {msg}"
        );
        assert!(
            msg.contains("expected `key: value`"),
            "a list item is not a missing description: {msg}"
        );
        assert!(
            !msg.contains("missing required field"),
            "a present list must not look like a missing field: {msg}"
        );
    }

    #[test]
    fn description_list_after_blank_or_comment_names_the_item() {
        let assert_names_item = |input: &str| {
            let err = parse_skill(input).expect_err(input);
            let msg = err.to_string();
            assert!(
                msg.contains("expected `key: value`") && msg.contains("- use foo: bar"),
                "{msg}"
            );
            assert!(!msg.contains("value is empty"), "{msg}");
        };
        assert_names_item(
            "\
---
name: n
description:

  - use foo: bar
---
body
",
        );
        assert_names_item(
            "\
---
name: n
description:
  # when
  - use foo: bar
---
body
",
        );
        let empty = parse_skill("---\nname: n\ndescription:\n---\nbody\n").expect_err("empty");
        assert!(
            empty.to_string().contains("description value is empty"),
            "{empty}"
        );
        let when = parse_skill(
            "\
---
name: n
description: d
when-to-use:

  - editing
---
body
",
        )
        .expect_err("when-to-use");
        let when_msg = when.to_string();
        assert!(
            when_msg.contains("expected `key: value`") && when_msg.contains("- editing"),
            "{when_msg}"
        );
    }

    #[test]
    fn name_and_bool_keys_name_a_following_list() {
        let assert_names_item = |input: &str, item: &str| {
            let err = parse_skill(input).expect_err(input);
            let msg = err.to_string();
            assert!(
                msg.contains("expected `key: value`") && msg.contains(item),
                "{msg}"
            );
            assert!(!msg.contains("value is empty"), "{msg}");
        };
        assert_names_item(
            "\
---
name:
  - my-skill
description: d
---
body
",
            "- my-skill",
        );
        assert_names_item(
            "\
---
name: n
description: d
user-invocable:
  - true
---
body
",
            "- true",
        );
        assert_names_item(
            "\
---
name: n
description: d
disable-model-invocation:
  - true
---
body
",
            "- true",
        );
        assert_names_item(
            "\
---
name: n
description: d
user_invocable:

  # later
  - true
---
body
",
            "- true",
        );
        let empty_name = parse_skill("---\nname:\ndescription: d\n---\nbody\n").expect_err("name");
        assert!(
            empty_name.to_string().contains("name value is empty"),
            "{empty_name}"
        );
        let empty_bool = parse_skill("---\nname: n\ndescription: d\nuser-invocable:\n---\nbody\n")
            .expect_err("bool");
        assert!(
            empty_bool
                .to_string()
                .contains("user_invocable value is empty"),
            "{empty_bool}"
        );
    }

    #[test]
    fn unquoted_null_and_tilde_are_empty_scalars() {
        for token in ["null", "Null", "NULL", "~"] {
            let description = format!("---\nname: n\ndescription: {token}\n---\nbody\n");
            let err = parse_skill(&description).expect_err(&description);
            assert!(
                err.to_string().contains("description value is empty"),
                "{token}: {err}"
            );
            let name = format!("---\nname: {token}\ndescription: d\n---\nbody\n");
            let err = parse_skill(&name).expect_err(&name);
            let msg = err.to_string();
            assert!(
                msg.contains("name value is empty"),
                "{token} must not fail the charset check: {msg}"
            );
            assert!(!msg.contains("lowercase alphanumeric"), "{token}: {msg}");
        }
        let quoted = parse_skill(
            "---\nname: \"null\"\ndescription: 'null'\nlicense: \"NULL\"\ncompatibility: '~'\n---\nbody\n",
        )
        .expect("quoted null is the word");
        assert_eq!(quoted.name, "null");
        assert_eq!(quoted.description, "null");
        assert_eq!(quoted.license.as_deref(), Some("NULL"));
        assert_eq!(quoted.compatibility.as_deref(), Some("~"));
        let word = parse_skill("---\nname: n\ndescription: nullnull\n---\nbody\n")
            .expect("nullnull is not a null token");
        assert_eq!(word.description, "nullnull");
        let quoted_tilde = parse_skill("---\nname: n\ndescription: \"~\"\n---\nbody\n")
            .expect("quoted tilde is the character");
        assert_eq!(quoted_tilde.description, "~");
        let quoted_name = parse_skill("---\nname: \"~\"\ndescription: d\n---\nbody\n")
            .expect_err("quoted tilde is not an empty name");
        assert!(
            quoted_name.to_string().contains("lowercase alphanumeric"),
            "{quoted_name}"
        );
    }

    #[test]
    fn optional_string_null_and_flow_are_not_stored_text() {
        for (key, field) in [
            ("license", "license"),
            ("compatibility", "compatibility"),
            ("allowed-tools", "allowed_tools"),
            ("allowed_tools", "allowed_tools"),
            ("argument-hint", "argument_hint"),
            ("argument_hint", "argument_hint"),
            ("when-to-use", "when_to_use"),
            ("when_to_use", "when_to_use"),
        ] {
            for token in ["null", "Null", "NULL", "~"] {
                let input = format!("---\nname: n\ndescription: d\n{key}: {token}\n---\nbody\n");
                let skill = parse_skill(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
                let got = match field {
                    "license" => skill.license,
                    "compatibility" => skill.compatibility,
                    "allowed_tools" => skill.allowed_tools,
                    "argument_hint" => skill.argument_hint,
                    "when_to_use" => skill.when_to_use,
                    other => panic!("{other}"),
                };
                assert!(got.is_none(), "{key}: {token} stored {got:?}");
            }
        }
        let quoted = parse_skill(
            "---\nname: n\ndescription: d\nlicense: \"null\"\nwhen-to-use: \"~\"\n---\nbody\n",
        )
        .expect("quoted null stays");
        assert_eq!(quoted.license.as_deref(), Some("null"));
        assert_eq!(quoted.when_to_use.as_deref(), Some("~"));

        for (input_key, raw) in [
            ("when-to-use", "[editing]"),
            ("license", "[MIT]"),
            ("argument-hint", "{a: b}"),
        ] {
            let input = format!("---\nname: n\ndescription: d\n{input_key}: {raw}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                msg.contains("must be a string, got: ") && msg.contains(raw),
                "{msg}"
            );
            assert_eq!(msg.lines().count(), 1, "{msg:?}");
        }
        let brackets =
            parse_skill("---\nname: n\ndescription: d\nwhen-to-use: \"see [one]\"\n---\nbody\n")
                .expect("quoted brackets");
        assert_eq!(brackets.when_to_use.as_deref(), Some("see [one]"));
        for key in ["argument-hint", "argument_hint"] {
            let input = format!("---\nname: n\ndescription: d\n{key}: [name]\n---\nbody\n");
            let skill = parse_skill(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(skill.argument_hint.as_deref(), Some("[name]"), "{key}");
        }

        for token in ["null", "NULL", "~"] {
            let input = format!("---\nname: n\ndescription: d\ntriggers: {token}\n---\nbody\n");
            let skill = parse_skill(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert!(skill.triggers.is_empty(), "{token}: {:?}", skill.triggers);
        }
        let quoted_trigger =
            parse_skill("---\nname: n\ndescription: d\ntriggers: [\"null\", git]\n---\nbody\n")
                .expect("quoted trigger null");
        assert_eq!(
            quoted_trigger.triggers,
            vec!["null".to_owned(), "git".to_owned()]
        );
        let skipped =
            parse_skill("---\nname: n\ndescription: d\ntriggers: [null, git]\n---\nbody\n")
                .expect("skip null item");
        assert_eq!(skipped.triggers, vec!["git".to_owned()]);
        let kept =
            parse_skill("---\nname: n\ndescription: d\ntriggers: [git, rebase]\n---\nbody\n")
                .expect("flow triggers");
        assert_eq!(kept.triggers, vec!["git".to_owned(), "rebase".to_owned()]);

        let list = parse_skill("---\nname: n\ndescription: d\nlicense:\n  - MIT\n---\nbody\n")
            .expect_err("license list");
        let list_msg = list.to_string();
        assert!(
            list_msg.contains("expected `key: value`") && list_msg.contains("- MIT"),
            "{list_msg}"
        );
    }

    #[test]
    fn unquoted_yaml_bool_words_are_not_stored_text() {
        for token in [
            "true", "false", "yes", "no", "on", "off", "TRUE", "Yes", "OFF",
        ] {
            let description = format!("---\nname: n\ndescription: {token}\n---\nbody\n");
            let err = parse_skill(&description).expect_err(&description);
            let msg = err.to_string();
            assert!(msg.contains("description value is empty"), "{token}: {msg}");
            assert!(!msg.contains("must be a string"), "{token}: {msg}");
            let name = format!("---\nname: {token}\ndescription: d\n---\nbody\n");
            let err = parse_skill(&name).expect_err(&name);
            let msg = err.to_string();
            assert!(
                msg.contains("name value is empty"),
                "{token} must not become a skill name: {msg}"
            );
            assert!(!msg.contains("lowercase alphanumeric"), "{token}: {msg}");
        }
        let commented = parse_skill("---\nname: n\ndescription: yes # placeholder\n---\nbody\n")
            .expect_err("comment");
        assert!(
            commented.to_string().contains("description value is empty"),
            "{commented}"
        );

        for (key, token) in [
            ("license", "true"),
            ("compatibility", "false"),
            ("allowed-tools", "yes"),
            ("allowed_tools", "no"),
            ("argument-hint", "on"),
            ("argument_hint", "off"),
            ("when-to-use", "TRUE"),
            ("when_to_use", "Yes"),
        ] {
            let input = format!("---\nname: n\ndescription: d\n{key}: {token}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                msg.contains(&format!("{key} must be a string, got: {token}")),
                "{key} {token}: {msg}"
            );
            assert_eq!(msg.lines().count(), 1, "{msg:?}");
        }

        let quoted = parse_skill(
            "---\nname: \"true\"\ndescription: \"false\"\nlicense: 'yes'\nwhen-to-use: \"on\"\nargument-hint: \"off\"\n---\nbody\n",
        )
        .expect("quoted bool words stay text");
        assert_eq!(quoted.name, "true");
        assert_eq!(quoted.description, "false");
        assert_eq!(quoted.license.as_deref(), Some("yes"));
        assert_eq!(quoted.when_to_use.as_deref(), Some("on"));
        assert_eq!(quoted.argument_hint.as_deref(), Some("off"));

        let phrase =
            parse_skill("---\nname: n\ndescription: true story\nlicense: yesman\n---\nbody\n")
                .expect("longer words are text");
        assert_eq!(phrase.description, "true story");
        assert_eq!(phrase.license.as_deref(), Some("yesman"));
        let digits =
            parse_skill("---\nname: n\ndescription: 1\nlicense: 0\n---\nbody\n").expect("digits");
        assert_eq!(digits.description, "1");
        assert_eq!(digits.license.as_deref(), Some("0"));

        let block = parse_skill("---\nname: n\ndescription: |\n  true\n---\nbody\n")
            .expect("block scalar is text");
        assert_eq!(block.description, "true");

        let flagged =
            parse_skill("---\nname: n\ndescription: d\nuser-invocable: false\n---\nbody\n")
                .expect("bool key");
        assert!(!flagged.user_invocable);

        let meta = parse_skill(
            "---\nname: n\ndescription: d\nmetadata: {author: true, version: 1}\n---\nbody\n",
        )
        .expect_err("flow metadata bool");
        assert!(
            meta.to_string()
                .contains("metadata value must be a string, got: true"),
            "{meta}"
        );
        let meta_block = parse_skill(
            "---\nname: n\ndescription: d\nmetadata:\n  author: YES\n  version: 1\n---\nbody\n",
        )
        .expect_err("block metadata bool");
        assert!(meta_block.to_string().contains("got: YES"), "{meta_block}");
        let meta_quoted = parse_skill(
            "---\nname: n\ndescription: d\nmetadata: {author: \"true\", version: 1}\n---\nbody\n",
        )
        .expect("quoted metadata bool word");
        assert_eq!(
            meta_quoted.metadata.get("author").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            meta_quoted.metadata.get("version").map(String::as_str),
            Some("1")
        );
        let meta_num =
            parse_skill("---\nname: n\ndescription: d\nmetadata:\n  version: 1\n---\nbody\n")
                .expect("digit metadata stays");
        assert_eq!(
            meta_num.metadata.get("version").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn nested_block_under_known_scalar_is_not_omitted() {
        for (key, nested) in [
            ("license", "spdx: MIT"),
            ("compatibility", "os: linux"),
            ("allowed-tools", "bash: git"),
            ("allowed_tools", "bash: git"),
            ("when-to-use", "task: review"),
            ("when_to_use", "task: review"),
            ("argument-hint", "name: file"),
            ("argument_hint", "name: file"),
        ] {
            let input = format!("---\nname: n\ndescription: d\n{key}:\n  {nested}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                msg.contains(&format!("{key} must be a string, got: {nested}")),
                "{key}: {msg}"
            );
            assert_eq!(msg.lines().count(), 1, "{msg:?}");
        }
        let commented = "\
---
name: n
description: d
license:
  # later
  spdx: MIT
---
body
";
        let err = parse_skill(commented).expect_err("comment before nested");
        assert!(
            err.to_string()
                .contains("license must be a string, got: spdx: MIT"),
            "{err}"
        );
        let blank = "\
---
name: n
description: d
license:

  spdx: MIT
---
body
";
        let err = parse_skill(blank).expect_err("blank before nested");
        assert!(err.to_string().contains("spdx: MIT"), "{err}");
        let nbsp = "---\nname: n\ndescription: d\nlicense:\n\u{00a0}spdx: MIT\n---\nbody\n";
        let err = parse_skill(nbsp).expect_err("nbsp indent");
        assert!(err.to_string().contains("spdx: MIT"), "{err}");
        let deeper = "\
---
name: n
description: d
compatibility:
  os:
    name: linux
---
body
";
        let err = parse_skill(deeper).expect_err("deeper");
        assert!(
            err.to_string()
                .contains("compatibility must be a string, got: os:"),
            "{err}"
        );
        let hostile = format!(
            "---\nname: n\ndescription: d\nlicense:\n  spdx: MIT{}\n---\nbody\n",
            "\u{2028}"
        );
        let err = parse_skill(&hostile).expect_err("line sep");
        let msg = err.to_string();
        assert!(!msg.contains('\u{2028}'), "{msg:?}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");

        let list = parse_skill("---\nname: n\ndescription: d\nlicense:\n  - MIT\n---\nbody\n")
            .expect_err("list stays the list error");
        let list_msg = list.to_string();
        assert!(
            list_msg.contains("expected `key: value`") && list_msg.contains("- MIT"),
            "{list_msg}"
        );

        let literal =
            parse_skill("---\nname: n\ndescription: d\nlicense: |\n  spdx: MIT\n---\nbody\n")
                .expect("literal block is a string");
        assert_eq!(literal.license.as_deref(), Some("spdx: MIT"));
        let quoted = parse_skill(
            "---\nname: n\ndescription: d\nlicense: \"spdx: MIT\"\ncompatibility: 'os: linux'\n---\nbody\n",
        )
        .expect("quoted colon stays");
        assert_eq!(quoted.license.as_deref(), Some("spdx: MIT"));
        assert_eq!(quoted.compatibility.as_deref(), Some("os: linux"));

        let author = "\
---
author:
  name: ada
name: n
description: d
---
body
";
        let err = parse_skill(author).expect_err("top-level author block");
        assert!(
            err.to_string().contains("metadata author must be a string"),
            "{err}"
        );
        let author_flow =
            parse_skill("---\nname: n\ndescription: d\nauthor: {name: ada}\n---\nbody\n")
                .expect_err("top-level author flow");
        assert!(
            author_flow
                .to_string()
                .contains("metadata value must be a string, got: {name: ada}"),
            "{author_flow}"
        );
        let author_text =
            parse_skill("---\nname: n\ndescription: d\nauthor: Ada Lovelace\n---\nbody\n")
                .expect("scalar author is still an unknown key");
        assert!(author_text.metadata.is_empty());
        let author_list =
            parse_skill("---\nname: n\ndescription: d\nauthor:\n  - ada\n---\nbody\n")
                .expect("author list stays ignored");
        assert!(author_list.metadata.is_empty());
        let author_quoted =
            parse_skill("---\nname: n\ndescription: d\nauthor: \"{name: ada}\"\n---\nbody\n")
                .expect("quoted author flow stays ignored");
        assert!(author_quoted.metadata.is_empty());
        let hooks =
            parse_skill("---\nname: n\ndescription: d\nhooks:\n  name: pre-commit\n---\nbody\n")
                .expect("hooks nested map still loads");
        assert_eq!(hooks.name, "n");
        let version =
            parse_skill("---\nname: n\ndescription: d\nversion:\n  major: 1\n---\nbody\n")
                .expect("unknown nested map still loads");
        assert_eq!(version.name, "n");
        assert!(version.metadata.is_empty());
    }

    #[test]
    fn nested_key_after_nonempty_scalar_is_not_dropped() {
        let prose = "\
---
name: foldreal
description: Review a pull request
  and check the title: it must match the issue.
---
body
";
        let err = parse_skill(prose).expect_err("colon sentence must not vanish");
        let msg = err.to_string();
        assert!(
            msg.contains(
                "description must be a string, got: and check the title: it must match the issue."
            ),
            "{msg}"
        );
        assert_eq!(msg.lines().count(), 1, "{msg:?}");

        let commented = "\
---
name: n
description: hello
  # later
  world: not-a-key
---
body
";
        let err = parse_skill(commented).expect_err("comment before nested");
        assert!(
            err.to_string()
                .contains("description must be a string, got: world: not-a-key"),
            "{err}"
        );

        let quoted = "\
---
name: n
description: \"hello\"
  world: not-a-key
---
body
";
        let err = parse_skill(quoted).expect_err("quoted description is closed");
        assert!(
            err.to_string()
                .contains("description must be a string, got: world: not-a-key"),
            "{err}"
        );

        let named = "\
---
name: review-pr
  aka: review
description: d
---
body
";
        let err = parse_skill(named).expect_err("name does not swallow a nested key");
        assert!(
            err.to_string()
                .contains("name must be a string, got: aka: review"),
            "{err}"
        );

        for (key, nested) in [
            ("license", "spdx: MIT"),
            ("compatibility", "os: linux"),
            ("allowed-tools", "bash: git"),
            ("allowed_tools", "bash: git"),
            ("when-to-use", "task: review"),
            ("when_to_use", "task: review"),
            ("argument-hint", "name: file"),
            ("argument_hint", "name: file"),
        ] {
            let input =
                format!("---\nname: n\ndescription: d\n{key}: MIT\n  {nested}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                msg.contains(&format!("{key} must be a string, got: {nested}")),
                "{key}: {msg}"
            );
            assert_eq!(msg.lines().count(), 1, "{msg:?}");
        }

        let triggers = "\
---
name: n
description: d
triggers: review
  code: review
---
body
";
        let err = parse_skill(triggers).expect_err("trigger scalar is closed");
        assert!(
            err.to_string()
                .contains("triggers must be a string, got: code: review"),
            "{err}"
        );
        let quoted_triggers = "\
---
name: n
description: d
triggers: \"review\"
  code: review
---
body
";
        let err = parse_skill(quoted_triggers).expect_err("quoted trigger is closed");
        assert!(
            err.to_string()
                .contains("triggers must be a string, got: code: review"),
            "{err}"
        );

        let wrapped = parse_skill(
            "---\nname: n\ndescription: d\ntriggers: pull request,\n  code review\n---\nbody\n",
        )
        .expect("wrapped triggers still split");
        assert_eq!(
            wrapped.triggers,
            vec!["pull request".to_owned(), "code review".to_owned()]
        );

        let url = parse_skill(
            "---\nname: n\ndescription: See the docs at\n  https://example.com/a:b more\n---\nbody\n",
        )
        .expect("url colon stays text");
        assert_eq!(
            url.description,
            "See the docs at https://example.com/a:b more"
        );

        let flow = parse_skill(
            "---\nname: n\ndescription: hello\n  {a: b}\nlicense: MIT\n  {spdx: MIT}\n---\nbody\n",
        )
        .expect("a flow map under a scalar is not folded and still loads");
        assert_eq!(flow.description, "hello");
        assert_eq!(flow.license.as_deref(), Some("MIT"));

        let flagged = "\
---
name: n
description: d
user-invocable: false
  note: internal only
---
body
";
        let err = parse_skill(flagged).expect_err("bool does not swallow a nested key");
        assert!(
            err.to_string()
                .contains("user_invocable must be a boolean, got: note: internal only"),
            "{err}"
        );
        let flagged_next = "\
---
name: n
description: d
user-invocable: false
  disable-model-invocation: true
---
body
";
        let err = parse_skill(flagged_next).expect_err("indented bool key must not vanish");
        assert!(
            err.to_string()
                .contains("user_invocable must be a boolean, got: disable-model-invocation: true"),
            "{err}"
        );

        let meta = "\
---
name: n
description: d
metadata: {author: ada}
  version: 1
---
body
";
        let err = parse_skill(meta).expect_err("flow metadata does not drop the next key");
        assert!(
            err.to_string()
                .contains("metadata value must be a string, got: version: 1"),
            "{err}"
        );
    }

    #[test]
    fn quoted_trigger_scalar_keeps_a_comma() {
        let quoted = parse_skill(
            "---\nname: n\ndescription: d\ntriggers: \"changelog, release\"\n---\nbody\n",
        )
        .expect("quoted trigger");
        assert_eq!(quoted.triggers, vec!["changelog, release".to_owned()]);
        let single = parse_skill(
            "---\nname: n\ndescription: d\ntriggers: 'changelog, release'\n---\nbody\n",
        )
        .expect("single quoted trigger");
        assert_eq!(single.triggers, vec!["changelog, release".to_owned()]);
        let split =
            parse_skill("---\nname: n\ndescription: d\ntriggers: changelog, release\n---\nbody\n")
                .expect("unquoted triggers");
        assert_eq!(
            split.triggers,
            vec!["changelog".to_owned(), "release".to_owned()]
        );
    }

    #[test]
    fn metadata_null_value_is_omitted() {
        let flow = parse_skill(
            "---\nname: n\ndescription: d\nmetadata: {author: null, version: 1}\n---\nbody\n",
        )
        .expect("flow metadata");
        assert!(
            !flow.metadata.contains_key("author"),
            "author stored {:?}",
            flow.metadata.get("author")
        );
        assert_eq!(flow.metadata.get("version").map(String::as_str), Some("1"));
        let quoted =
            parse_skill("---\nname: n\ndescription: d\nmetadata: {author: \"null\"}\n---\nbody\n")
                .expect("quoted metadata null");
        assert_eq!(
            quoted.metadata.get("author").map(String::as_str),
            Some("null")
        );
        let block = parse_skill(
            "---\nname: n\ndescription: d\nmetadata:\n  author: ~\n  version: 1\n---\nbody\n",
        )
        .expect("block metadata");
        assert!(
            !block.metadata.contains_key("author"),
            "{:?}",
            block.metadata
        );
        assert_eq!(block.metadata.get("version").map(String::as_str), Some("1"));
        let err = parse_skill("---\nname: n\ndescription: d\nmetadata: null\n---\nbody\n")
            .expect_err("scalar metadata");
        assert!(err.to_string().contains("must be a map"), "{err}");
    }

    #[test]
    fn nested_metadata_is_not_a_flat_string_map() {
        let nested = "\
---
name: nested
description: d
metadata:
  author:
    name: ada
  tags:
    - one
---
body
";
        let err = parse_skill(nested).expect_err("nested metadata");
        let msg = err.to_string();
        assert!(msg.contains("metadata author must be a string"), "{msg}");
        let list = "\
---
name: tagged
description: d
metadata:
  tags:
    - one
---
body
";
        let list_err = parse_skill(list).expect_err("metadata list");
        assert!(
            list_err.to_string().contains("must be a string"),
            "{list_err}"
        );
        let bare_list = "\
---
name: bare
description: d
metadata:
  - one
---
body
";
        let bare_err = parse_skill(bare_list).expect_err("metadata sequence");
        assert!(
            bare_err.to_string().contains("must be a string")
                && bare_err.to_string().contains("- one"),
            "{bare_err}"
        );
        let flat = parse_skill(
            "---\nname: flat\ndescription: d\nmetadata:\n  author: ada\n  version: \"\"\n---\nbody\n",
        )
        .expect("flat metadata");
        assert_eq!(flat.metadata.get("author").map(String::as_str), Some("ada"));
        assert_eq!(flat.metadata.get("version").map(String::as_str), Some(""));
        let flow = parse_skill(
            "---\nname: flow\ndescription: d\nmetadata: {author: {name: ada}, version: 1}\n---\nbody\n",
        )
        .expect_err("nested flow metadata");
        assert!(
            flow.to_string().contains("metadata value must be a string")
                && flow.to_string().contains("{name: ada}"),
            "{flow}"
        );
        let block_flow = parse_skill(
            "---\nname: block\ndescription: d\nmetadata:\n  author: {name: ada}\n---\nbody\n",
        )
        .expect_err("block flow metadata");
        assert!(
            block_flow.to_string().contains("{name: ada}"),
            "{block_flow}"
        );
        let quoted = parse_skill(
            "---\nname: quoted\ndescription: d\nmetadata: {author: \"{name: ada}\"}\n---\nbody\n",
        )
        .expect("quoted flow text");
        assert_eq!(
            quoted.metadata.get("author").map(String::as_str),
            Some("{name: ada}")
        );
    }

    #[test]
    fn description_flow_collection_is_invalid_yaml() {
        let assert_flow = |raw: &str| {
            let input = format!("---\nname: n\ndescription: {raw}\n---\nbody\n");
            let err = parse_skill(&input).expect_err(&input);
            let msg = err.to_string();
            assert!(
                msg.contains("description must be a string, got: ") && msg.contains(raw),
                "{msg}"
            );
            assert!(!msg.contains("value is empty"), "{msg}");
            assert_eq!(msg.lines().count(), 1, "{msg:?}");
        };
        assert_flow("[one, two]");
        assert_flow("{a: b}");
        assert_flow("[]");
        assert_flow("{}");
        let hostile = format!(
            "---\nname: n\ndescription: [{}\u{2028}]\n---\nbody\n",
            "one"
        );
        let err = parse_skill(&hostile).expect_err(&hostile);
        let msg = err.to_string();
        assert!(!msg.contains('\u{2028}'), "{msg:?}");
        assert!(msg.contains("[one?]"), "{msg}");
        let quoted =
            parse_skill("---\nname: n\ndescription: \"see [one]\"\nlicense: '{a: b}'\n---\nbody\n")
                .expect("quoted brackets stay text");
        assert_eq!(quoted.description, "see [one]");
        assert_eq!(quoted.license.as_deref(), Some("{a: b}"));
        let quoted_flow = parse_skill("---\nname: n\ndescription: \"[one, two]\"\n---\nbody\n")
            .expect("a quoted flow is still a string");
        assert_eq!(quoted_flow.description, "[one, two]");
        let noted = parse_skill("---\nname: n\ndescription: [one, two] # note\n---\nbody\n")
            .expect_err("comment");
        assert!(
            noted
                .to_string()
                .contains("description must be a string, got: [one, two]"),
            "{noted}"
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
    fn unquoted_trailing_quotes_stay_on_the_scalar() {
        let skill = parse_skill(
            "---\nname: quotes\ndescription: Use when the user says \"deploy\"\nlicense: MIT'\n---\nbody\n",
        )
        .expect("parse");
        assert_eq!(skill.description, "Use when the user says \"deploy\"");
        assert_eq!(skill.license.as_deref(), Some("MIT'"));
    }

    #[test]
    fn quoted_yaml_escapes_decode() {
        let skill = parse_skill(
            "---\nname: esc\ndescription: \"Run \\\"cargo test\\\" first\"\nwhen-to-use: 'It''s time to ship'\n---\nb\n",
        )
        .expect("parse");
        assert_eq!(skill.description, "Run \"cargo test\" first");
        assert_eq!(skill.when_to_use.as_deref(), Some("It's time to ship"));
    }

    #[test]
    fn unknown_frontmatter_keys_skips_zero_indent_sequence_items() {
        let input = "\
---
name: lists
description: zero indent list
triggers:
- alpha
- \"ratio: 3\"
---
body
";
        assert!(
            unknown_frontmatter_keys(input).is_empty(),
            "keys={:?}",
            unknown_frontmatter_keys(input)
        );
        let skill = parse_skill(input).expect("parse");
        assert_eq!(skill.triggers, ["alpha".to_owned(), "ratio: 3".to_owned()]);
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
    fn parse_skill_nested_unknown_map_does_not_overwrite_top_level() {
        // Host-extension maps are ignored. Indented name / bool keys
        // must not become top-level (CLI load / MCP skills_load).
        let input = "\
---
name: wanted
description: docs
user-invocable: true
disable-model-invocation: false
metadata:
  name: meta-name
hooks:
  name: pre-commit
  user-invocable: false
  disable-model-invocation: true
  description: nested
---
BODY
";
        let skill = parse_skill(input).expect("host nested map must still load");
        assert_eq!(skill.name, "wanted");
        assert_eq!(skill.description, "docs");
        assert!(
            skill.user_invocable,
            "nested user-invocable must not flip the top-level value"
        );
        assert!(
            !skill.disable_model_invocation,
            "nested disable-model-invocation must not flip the top-level value"
        );
        assert_eq!(
            skill.metadata.get("name").map(String::as_str),
            Some("meta-name")
        );
        assert!(skill.content.contains("BODY"));
        assert_eq!(
            peek_frontmatter_name(input).as_deref(),
            Some("wanted"),
            "peek must keep the top-level name"
        );
    }

    #[test]
    fn parse_skill_inline_flow_metadata_is_kept() {
        // Block `metadata:` is already collected. After SkillSummary
        // emits the map, a flow `{k: v}` form must not silently drop
        // author/version (leftover analog of PR 182).
        let input = "\
---
name: annotated
description: docs
metadata: {author: A & B, version: '1.0'}
---
BODY
";
        let skill = parse_skill(input).expect("flow-map metadata must load");
        assert_eq!(
            skill.metadata.get("author").map(String::as_str),
            Some("A & B"),
            "inline metadata author: {:?}",
            skill.metadata
        );
        assert_eq!(
            skill.metadata.get("version").map(String::as_str),
            Some("1.0"),
            "inline metadata version: {:?}",
            skill.metadata
        );
        let load = crate::activate::format_load_message(
            &skill,
            "",
            crate::activate::FormatOptions::default(),
        );
        let header = load.split("\n---\n").next().expect("header");
        assert!(
            header.contains("Metadata: author=A & B, version=1.0\n"),
            "load must print flow-map metadata: {load}"
        );

        let garbage = "\
---
name: annotated
description: docs
metadata: not-a-map
---
BODY
";
        let err = parse_skill(garbage).expect_err("scalar metadata must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("metadata") && !msg.contains('\u{2028}'),
            "scalar metadata must name the field and stay one line: {msg}"
        );

        let quoted = "\
---
name: annotated
description: docs
metadata: {\"author\": \"A & B\"}
---
BODY
";
        let quoted_skill = parse_skill(quoted).expect("quoted flow-map keys must load");
        assert_eq!(
            quoted_skill.metadata.get("author").map(String::as_str),
            Some("A & B"),
            "quoted metadata key must peel quotes: {:?}",
            quoted_skill.metadata
        );

        let block_quoted = "\
---
name: annotated
description: docs
metadata:
  \"author\": \"A & B\"
---
BODY
";
        let block_skill = parse_skill(block_quoted).expect("quoted block metadata keys must load");
        assert_eq!(
            block_skill.metadata.get("author").map(String::as_str),
            Some("A & B"),
            "quoted block metadata key must peel quotes: {:?}",
            block_skill.metadata
        );
    }

    #[test]
    fn parse_skill_inline_flow_triggers_are_kept() {
        // Comma-split inline triggers are already collected. After
        // SkillSummary emits the list, a flow `[a, b]` form must not
        // keep the brackets as tokens (leftover analog of PR 183).
        let input = "\
---
name: triggered
description: docs
triggers: [git, rebase]
---
BODY
";
        let skill = parse_skill(input).expect("flow-list triggers must load");
        assert_eq!(
            skill.triggers,
            vec!["git", "rebase"],
            "flow-list triggers must peel brackets: {:?}",
            skill.triggers
        );
        let load = crate::activate::format_load_message(
            &skill,
            "",
            crate::activate::FormatOptions::default(),
        );
        let header = load.split("\n---\n").next().expect("header");
        assert!(
            header.contains("Triggers: git, rebase\n"),
            "load must print flow-list triggers: {load}"
        );

        let quoted = "\
---
name: triggered
description: docs
triggers: [\"A & B\", 'own CI']
---
BODY
";
        let quoted_skill = parse_skill(quoted).expect("quoted flow-list items must load");
        assert_eq!(
            quoted_skill.triggers,
            vec!["A & B", "own CI"],
            "quoted flow-list items must peel quotes: {:?}",
            quoted_skill.triggers
        );

        let empty = "\
---
name: triggered
description: docs
triggers: []
---
BODY
";
        let empty_skill = parse_skill(empty).expect("empty flow-list must load");
        assert!(
            empty_skill.triggers.is_empty(),
            "empty flow-list must not invent a token: {:?}",
            empty_skill.triggers
        );
    }

    #[test]
    fn parse_skill_flow_keeps_comma_inside_quotes() {
        let meta = parse_skill(
            "---\nname: meta-comma\ndescription: d\nmetadata: {author: \"Doe, Jane\", version: \"1\"}\n---\nbody\n",
        )
        .expect("quoted comma in flow metadata must load");
        assert_eq!(
            meta.metadata.get("author").map(String::as_str),
            Some("Doe, Jane")
        );
        assert_eq!(meta.metadata.get("version").map(String::as_str), Some("1"));

        let single = parse_skill(
            "---\nname: meta-comma\ndescription: d\nmetadata: {author: 'Doe, Jane'}\n---\nbody\n",
        )
        .expect("single-quoted comma in flow metadata must load");
        assert_eq!(
            single.metadata.get("author").map(String::as_str),
            Some("Doe, Jane")
        );

        let escaped = parse_skill(
            "---\nname: meta-comma\ndescription: d\nmetadata: {note: \"say \\\"hi, there\\\"\", version: \"1\"}\n---\nbody\n",
        )
        .expect("escaped quote and comma in flow metadata must load");
        assert_eq!(
            escaped.metadata.get("note").map(String::as_str),
            Some("say \"hi, there\"")
        );
        assert_eq!(
            escaped.metadata.get("version").map(String::as_str),
            Some("1")
        );

        let triggers = parse_skill(
            "---\nname: trig-comma\ndescription: d\ntriggers: [\"code, review\", git]\n---\nbody\n",
        )
        .expect("quoted comma in flow triggers must load");
        assert_eq!(triggers.triggers, vec!["code, review", "git"]);
    }

    #[test]
    fn parse_skill_flow_rejects_junk_after_quoted_scalar() {
        let meta = parse_skill(
            "---\nname: misscomma\ndescription: d\nmetadata: {author: \"Doe, Jane\" version: \"1\"}\n---\nbody\n",
        )
        .expect_err("missing comma must not fold the next pair into author");
        let meta_msg = meta.to_string();
        assert!(
            meta_msg.contains("metadata value must be a string") && meta_msg.contains("version"),
            "{meta_msg}"
        );

        let triggers = parse_skill(
            "---\nname: misscomma\ndescription: d\ntriggers: [\"code, review\" git]\n---\nbody\n",
        )
        .expect_err("missing comma must not glue two trigger scalars");
        let trig_msg = triggers.to_string();
        assert!(
            trig_msg.contains("trigger must be one scalar") && trig_msg.contains("git"),
            "{trig_msg}"
        );

        let kept = parse_skill(
            "---\nname: obrien\ndescription: d\nmetadata: {author: 'Doe, O''Brien', version: '1'}\n---\nbody\n",
        )
        .expect("escaped single quote inside a quoted comma must load");
        assert_eq!(
            kept.metadata.get("author").map(String::as_str),
            Some("Doe, O'Brien")
        );
        assert_eq!(kept.metadata.get("version").map(String::as_str), Some("1"));
    }

    #[test]
    fn peek_frontmatter_name_ignores_indented_name() {
        let input = "\
---
description: docs
hooks:
  name: pre-commit
---
BODY
";
        assert!(
            matches!(
                parse_skill(input),
                Err(ParseError::MissingField(ref f)) if f == "name"
            ),
            "missing top-level name still fails"
        );
        assert_eq!(
            peek_frontmatter_name(input),
            None,
            "nested name is not a skill identity for load/why peel"
        );
    }

    #[test]
    fn parse_skill_tab_indented_hooks_name_does_not_overwrite() {
        let input = "\
---
name: wanted
description: docs
hooks:
\tname: pre-commit
\tuser-invocable: false
---
BODY
";
        let skill = parse_skill(input).expect("tab-indented hooks map must still load");
        assert_eq!(skill.name, "wanted");
        assert!(
            skill.user_invocable,
            "tab-indented user-invocable must not flip the omitted default"
        );
        assert_eq!(peek_frontmatter_name(input).as_deref(), Some("wanted"));
    }

    #[test]
    fn parse_skill_nested_metadata_under_hooks_is_not_top_level_metadata() {
        let input = "\
---
name: wanted
description: docs
hooks:
  metadata:
    name: nested-meta
    author: nest
---
BODY
";
        let skill = parse_skill(input).expect("nested metadata under hooks must still load");
        assert_eq!(skill.name, "wanted");
        assert!(
            skill.metadata.is_empty(),
            "hooks.metadata is not top-level metadata: {:?}",
            skill.metadata
        );
        assert_eq!(peek_frontmatter_name(input).as_deref(), Some("wanted"));
    }

    #[test]
    fn parse_skill_indented_name_under_triggers_does_not_overwrite() {
        let input = "\
---
name: wanted
description: docs
triggers:
  - git
  name: evil
---
BODY
";
        let skill = parse_skill(input).expect("indented name under triggers must still load");
        assert_eq!(skill.name, "wanted");
        assert_eq!(skill.triggers, vec!["git"]);
        assert_eq!(peek_frontmatter_name(input).as_deref(), Some("wanted"));
    }

    #[test]
    fn parse_skill_unicode_indent_hooks_name_does_not_overwrite() {
        for indent in ["\u{00a0}", "\u{3000}", "\u{2003}", "\u{000b}", "\u{000c}"] {
            let input = format!(
                "---\nname: wanted\ndescription: docs\nhooks:\n{indent}name: pre-commit\n---\nBODY\n"
            );
            let skill = parse_skill(&input).unwrap_or_else(|e| {
                panic!("unicode indent {indent:?} hooks.name must still load: {e}")
            });
            assert_eq!(
                skill.name, "wanted",
                "unicode indent {indent:?} hooks.name must not overwrite"
            );
            assert_eq!(
                peek_frontmatter_name(&input).as_deref(),
                Some("wanted"),
                "peek unicode indent {indent:?}"
            );
        }
        let missing = "\
---
description: docs
hooks:
\u{00a0}name: pre-commit
---
BODY
";
        assert!(
            matches!(
                parse_skill(missing),
                Err(ParseError::MissingField(ref f)) if f == "name"
            ),
            "unicode-indented name is not a top-level name"
        );
        assert_eq!(
            peek_frontmatter_name(missing),
            None,
            "peek must ignore unicode-indented name"
        );
        assert!(
            !unknown_frontmatter_keys(
                "---\nname: wanted\ndescription: docs\nhooks:\n\u{00a0}host-only: x\n---\n"
            )
            .iter()
            .any(|k| k == "host-only"),
            "unicode-indented host key is not a top-level unknown"
        );
    }

    #[test]
    fn parse_skill_unicode_indent_nested_metadata_is_not_in_metadata() {
        let input = "\
---
name: wanted
description: docs
hooks:
\u{00a0}metadata:
    name: nested-meta
    author: nest
---
BODY
";
        let skill = parse_skill(input).expect("unicode-indented hooks.metadata must still load");
        assert_eq!(skill.name, "wanted");
        assert!(
            skill.metadata.is_empty(),
            "unicode-indented hooks.metadata is not top-level: {:?}",
            skill.metadata
        );
    }

    #[test]
    fn parse_skill_block_scalar_keeps_indented_name_line() {
        let input = "\
---
name: wanted
description: |
  line one
  name: not-a-key
\u{00a0}name: also-not-a-key
---
BODY
";
        let skill = parse_skill(input).expect("block scalar must keep indented lines");
        assert_eq!(skill.name, "wanted");
        assert_eq!(
            skill.description,
            "line one\nname: not-a-key\nname: also-not-a-key"
        );
    }

    #[test]
    fn split_frontmatter_returns_yaml_and_body() {
        let (yaml, body) =
            split_frontmatter("---\nname: demo\ndescription: d\n---\nhello\n").expect("split");
        assert_eq!(yaml, "name: demo\ndescription: d");
        assert_eq!(body, "hello\n");
        assert!(split_frontmatter("no delimiters").is_none());
        assert!(split_frontmatter("---\nname: demo\n").is_none());
        let (yaml_crlf, body_crlf) =
            split_frontmatter("---\r\nname: demo\r\n---\r\nbody\r\n").expect("crlf");
        assert_eq!(yaml_crlf, "name: demo\r");
        assert_eq!(body_crlf, "body\r\n");
        let skill = parse_skill("---\nname: demo\ndescription: d\n---\nhello\n").expect("parse");
        assert_eq!(skill.content, "hello\n");
        assert_eq!(
            peek_frontmatter_name("---\nname: demo\ndescription: d\n---\nhello\n").as_deref(),
            Some("demo")
        );
        assert!(
            unknown_frontmatter_keys("---\nname: demo\ndescription: d\nhost-only: x\n---\n")
                .contains(&"host-only".to_owned())
        );
    }

    #[test]
    fn empty_fences_are_frontmatter_without_a_name() {
        for input in [
            "---\n---\n# Body\n",
            "---\n\n---\n# Body\n",
            "---\r\n---\r\n# Body\n",
        ] {
            let err = parse_skill(input).expect_err(input);
            assert!(
                matches!(err, ParseError::MissingField(ref field) if field == "name"),
                "{input:?} -> {err}"
            );
            assert!(peek_frontmatter_name(input).is_none(), "{input:?}");
            let (yaml, body) = split_frontmatter(input).expect("fences split");
            assert!(yaml.trim().is_empty(), "{input:?} yaml={yaml:?}");
            assert!(body.contains("Body"), "{input:?} body={body:?}");
        }
        let spaced = parse_skill("---\n\nname: demo\ndescription: d\n\n---\nhello\n")
            .expect("blank lines inside fences");
        assert_eq!(spaced.name, "demo");
        assert_eq!(spaced.content, "hello\n");
    }

    #[test]
    fn split_frontmatter_strips_utf8_bom() {
        let input = "\u{feff}---\nname: demo\ndescription: d\n---\nhello\n";
        let (yaml, body) = split_frontmatter(input).expect("bom open");
        assert_eq!(yaml, "name: demo\ndescription: d");
        assert_eq!(body, "hello\n");
        let skill = parse_skill(input).expect("parse bom");
        assert_eq!(skill.name, "demo");
        assert_eq!(skill.content, "hello\n");
        assert_eq!(
            peek_frontmatter_name(input).as_deref(),
            Some("demo"),
            "peek must share the BOM strip"
        );
        assert!(
            unknown_frontmatter_keys(
                "\u{feff}---\nname: demo\ndescription: d\nhost-only: x\n---\n"
            )
            .contains(&"host-only".to_owned())
        );
        let spaced = "  \u{feff}---\nname: demo\ndescription: d\n---\nhello\n";
        assert!(
            split_frontmatter(spaced).is_some(),
            "BOM after leading spaces is still an open"
        );
        assert!(split_frontmatter("\u{feff}no delimiters").is_none());
        assert!(split_frontmatter("\u{feff}---\nname: demo\n").is_none());
    }

    #[test]
    fn split_frontmatter_paths_agree_on_fence_shapes() {
        let cases: &[(&str, bool, &str)] = &[
            (
                "---\nname: demo\ndescription: d\n---\nhello\n",
                true,
                "plain fences",
            ),
            (
                "---\r\nname: demo\ndescription: d\r\n---\r\nhello\r\n",
                true,
                "crlf fences",
            ),
            (
                "--- # open\nname: demo\ndescription: d\n--- # close\nhello\n",
                true,
                "comment after fence",
            ),
            (
                "---x\nname: demo\ndescription: d\n---\nhello\n",
                false,
                "stray char on open",
            ),
            (
                "----\nname: demo\ndescription: d\n---\nhello\n",
                false,
                "four-dash open",
            ),
            (
                "---\nname: demo\ndescription: d\n---x\nhello\n",
                false,
                "stray char on close",
            ),
            (
                "---\nname: demo\ndescription: d\n----\nhello\n",
                false,
                "four-dash close",
            ),
            ("---\nname: demo\ndescription: d\n", false, "no close"),
            (
                "---\nname: demo\ndescription: d\n---\n---\nmore\n",
                true,
                "body starts with ---",
            ),
        ];
        for &(input, expect_some, label) in cases {
            let split = split_frontmatter(input);
            assert_eq!(
                split.is_some(),
                expect_some,
                "split_frontmatter {label}: {input:?}"
            );
            if expect_some {
                assert!(
                    parse_skill(input).is_ok(),
                    "parse_skill {label}: {:?}",
                    parse_skill(input).err()
                );
                assert_eq!(
                    peek_frontmatter_name(input).as_deref(),
                    Some("demo"),
                    "peek {label}"
                );
            } else {
                assert!(
                    matches!(parse_skill(input), Err(ParseError::MissingFrontmatter)),
                    "parse_skill {label} must be MissingFrontmatter: {:?}",
                    parse_skill(input).err()
                );
                assert!(
                    peek_frontmatter_name(input).is_none(),
                    "peek {label} must miss"
                );
                assert!(
                    unknown_frontmatter_keys(input).is_empty(),
                    "unknown keys {label} must not invent a YAML block"
                );
            }
        }
        let body_dash = parse_skill("---\nname: demo\ndescription: d\n---\n---\nmore\n")
            .expect("body starting with ---");
        assert_eq!(body_dash.content, "---\nmore\n");
    }

    #[test]
    fn unknown_block_sequence_is_ignored_triggers_still_collect() {
        let tags_indent = "\
---
name: unkindent
description: unknown key indented list
tags:
  - alpha
---
body
";
        let tags_flat = "\
---
name: unkflat
description: unknown key flat list
tags:
- alpha
---
body
";
        let trig_indent = "\
---
name: trigindent
description: triggers indented list
triggers:
  - alpha
---
body
";
        let trig_flat = "\
---
name: trigflat
description: triggers flat list
triggers:
- alpha
---
body
";
        let tags_flow = "\
---
name: tagsflow
description: unknown key flow list
tags: [alpha]
---
body
";
        let license_seq = "\
---
name: licseq
description: known scalar as a list
license:
  - MIT
---
body
";
        for input in [tags_indent, tags_flat, tags_flow] {
            let skill = parse_skill(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert!(
                skill.triggers.is_empty(),
                "tags sequence is not triggers: {input}"
            );
        }
        assert_eq!(
            parse_skill(trig_indent).expect("trig indent").triggers,
            ["alpha".to_owned()]
        );
        assert_eq!(
            parse_skill(trig_flat).expect("trig flat").triggers,
            ["alpha".to_owned()]
        );
        let err = parse_skill(license_seq).expect_err(license_seq);
        assert!(
            matches!(err, ParseError::InvalidYaml(_)) && err.to_string().contains("- MIT"),
            "sequence under license must stay InvalidYaml: {err}"
        );
    }

    #[test]
    fn closing_fence_with_stray_char_is_not_body_prefix() {
        let input = "\
---
name: earlyclose
description: closing fence with stray char
---x
real body
";
        assert!(
            matches!(parse_skill(input), Err(ParseError::MissingFrontmatter)),
            "---x must not close frontmatter: {:?}",
            parse_skill(input).err()
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
    }

    #[test]
    fn parse_skill_malformed_line_hostile_token_is_sanitized() {
        // A YAML line with no colon is interpolated into InvalidYaml.
        // U+2028 / U+2014 must not leak (PR 113/119 covered format tokens
        // and later PRs covered hyphen bools, not this path).
        let input = "---\nname: demo\ndescription: d\nfoo\u{2028}bar\u{2014}baz\n---\nbody\n";
        let err = parse_skill(input).expect_err(input);
        let msg = err.to_string();
        assert!(
            matches!(err, ParseError::InvalidYaml(_)) && msg.contains("expected `key: value`"),
            "malformed line must stay InvalidYaml: {msg}"
        );
        assert_eq!(
            msg.lines().count(),
            1,
            "malformed-line error must stay one line: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2028}'),
            "U+2028 must not leak from malformed line: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2014}'),
            "em dash must not leak from malformed line: {msg:?}"
        );
        assert!(
            msg.contains("foo?bar-baz"),
            "hostile token must be sanitized in place: {msg}"
        );
    }

    #[test]
    fn parse_skill_empty_block_hostile_key_is_sanitized() {
        let input = "---\nname: demo\ndescription: d\nfoo\u{2028}bar\u{2014}: |\n---\nbody\n";
        let err = parse_skill(input).expect_err(input);
        let msg = err.to_string();
        assert!(
            matches!(err, ParseError::InvalidYaml(_)) && msg.contains("block scalar is empty"),
            "empty block must stay InvalidYaml: {msg}"
        );
        assert_eq!(
            msg.lines().count(),
            1,
            "empty-block error must stay one line: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2028}'),
            "U+2028 must not leak from empty-block key: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2014}'),
            "em dash must not leak from empty-block key: {msg:?}"
        );
        assert!(
            msg.contains("foo?bar-"),
            "hostile key must be sanitized in place: {msg}"
        );
    }

    #[test]
    fn parse_skill_empty_folded_block_hostile_key_is_sanitized() {
        let input = "---\nname: demo\ndescription: d\nfoo\u{2028}bar\u{2014}: >\n---\nbody\n";
        let err = parse_skill(input).expect_err(input);
        let msg = err.to_string();
        assert!(
            matches!(err, ParseError::InvalidYaml(_)) && msg.contains("block scalar is empty"),
            "empty folded block must stay InvalidYaml: {msg}"
        );
        assert_eq!(
            msg.lines().count(),
            1,
            "empty folded-block error must stay one line: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2028}'),
            "U+2028 must not leak from empty folded-block key: {msg:?}"
        );
        assert!(
            !msg.contains('\u{2014}'),
            "em dash must not leak from empty folded-block key: {msg:?}"
        );
        assert!(
            msg.contains("foo?bar-"),
            "hostile folded-block key must be sanitized in place: {msg}"
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
