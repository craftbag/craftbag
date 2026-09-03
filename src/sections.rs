//! Split a SKILL.md body into flat, addressable heading sections.
//!
//! Matching agentskills-core: slug keys, `-2` / `-3` on collision, preamble
//! before the first heading, and a parent does not include child headings.
//! Token estimate is 4 characters per token. Below
//! [`WHOLE_BODY_CHEAPER_TOKENS`] the whole body is cheaper than outline
//! then section. Does not read `scripts/` / `references/` / `assets/`.

use crate::sanitize_error_token;

/// Cheap heuristic used by agentskills-core `estimate_tokens`.
pub const CHARS_PER_TOKEN: usize = 4;
/// Below this, outline-then-section costs more than the whole body.
pub const WHOLE_BODY_CHEAPER_TOKENS: usize = 1000;

/// One heading (or the preamble) plus the text until the next heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSection {
    /// Slug key (`setup`, `setup-2`, `preamble`).
    pub key: String,
    /// Heading text without `#`, or `Preamble`.
    pub title: String,
    /// 0 for preamble, 1-6 for ATX headings.
    pub level: u8,
    /// Heading line plus body, or preamble text only.
    pub body: String,
}

/// Section keys and token costs for a SKILL.md body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillOutline {
    pub sections: Vec<SkillSectionMeta>,
    pub whole_tokens: usize,
    pub whole_body_is_cheaper: bool,
}

/// One outline row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSectionMeta {
    pub key: String,
    pub title: String,
    pub tokens: usize,
}

/// `ceil(chars / 4)`. Empty text is 0.
pub fn estimate_tokens(text: &str) -> usize {
    let n = text.chars().count();
    if n == 0 {
        0
    } else {
        n.div_ceil(CHARS_PER_TOKEN)
    }
}

/// Split `content` into flat sections. Parent text stops at the next
/// heading of any level. ATX lines inside a fenced ` ``` ` / `~~~ `
/// block are not headings.
pub fn split_sections(content: &str) -> Vec<SkillSection> {
    let mut headings: Vec<(usize, u8, String)> = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    for line in content.split_inclusive('\n') {
        if let Some((ch, n)) = fence {
            if closes_fence(line, ch, n) {
                fence = None;
            }
        } else if let Some(open) = opening_fence(line) {
            fence = Some(open);
        } else if let Some((level, title)) = atx_heading(line) {
            headings.push((offset, level, title));
        }
        offset += line.len();
    }

    let mut out = Vec::new();
    let mut used: Vec<String> = Vec::new();

    if headings.is_empty() {
        let body = content.trim();
        if !body.is_empty() {
            out.push(SkillSection {
                key: unique_key("preamble", &mut used),
                title: "Preamble".to_owned(),
                level: 0,
                body: body.to_owned(),
            });
        }
        return out;
    }

    let first_at = headings[0].0;
    let preamble = content[..first_at].trim();
    if !preamble.is_empty() {
        out.push(SkillSection {
            key: unique_key("preamble", &mut used),
            title: "Preamble".to_owned(),
            level: 0,
            body: preamble.to_owned(),
        });
    }

    for (i, &(start, level, ref title)) in headings.iter().enumerate() {
        let end = headings
            .get(i + 1)
            .map(|(next, _, _)| *next)
            .unwrap_or(content.len());
        let body = content[start..end].trim_end().to_owned();
        let slug = slugify(title);
        out.push(SkillSection {
            key: unique_key(&slug, &mut used),
            title: title.clone(),
            level,
            body,
        });
    }
    out
}

/// Outline rows plus the cheaper-than-section hint.
pub fn outline_of(content: &str) -> SkillOutline {
    let sections = split_sections(content);
    let whole_tokens = estimate_tokens(content.trim());
    SkillOutline {
        sections: sections
            .iter()
            .map(|s| SkillSectionMeta {
                key: s.key.clone(),
                title: s.title.clone(),
                tokens: estimate_tokens(&s.body),
            })
            .collect(),
        whole_tokens,
        whole_body_is_cheaper: whole_tokens < WHOLE_BODY_CHEAPER_TOKENS,
    }
}

/// Body for `key`, or an error that lists the keys that exist.
pub fn skill_section(content: &str, key: &str) -> Result<SkillSection, String> {
    let sections = split_sections(content);
    match sections.iter().find(|s| s.key == key) {
        Some(found) => Ok(found.clone()),
        None => Err(unknown_section_message(key, &sections)),
    }
}

/// One-line miss. `key` is sanitized like other host-echoed tokens.
pub fn unknown_section_message(key: &str, sections: &[SkillSection]) -> String {
    let shown = sanitize_error_token(key);
    if sections.is_empty() {
        return format!("unknown section: {shown} (no headings)");
    }
    let keys: Vec<String> = sections
        .iter()
        .map(|s| sanitize_error_token(&s.key))
        .collect();
    format!("unknown section: {shown} (available: {})", keys.join(", "))
}

/// CommonMark opening fence: 0-3 spaces, then 3+ backticks or tildes.
fn opening_fence(line: &str) -> Option<(u8, usize)> {
    let rest = trim_line_end(line);
    let rest = skip_atx_indent(rest)?;
    let bytes = rest.as_bytes();
    let first = *bytes.first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let n = bytes.iter().take_while(|&&b| b == first).count();
    if n < 3 {
        return None;
    }
    Some((first, n))
}

/// Close only on the same fence character, at least as long, then blanks.
fn closes_fence(line: &str, ch: u8, n: usize) -> bool {
    let rest = trim_line_end(line);
    let Some(rest) = skip_atx_indent(rest) else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.first().copied() != Some(ch) {
        return false;
    }
    let m = bytes.iter().take_while(|&&b| b == ch).count();
    if m < n {
        return false;
    }
    rest[m..].trim().is_empty()
}

fn trim_line_end(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

/// Up to 3 leading spaces, like ATX. Four spaces is indented code, not a fence.
fn skip_atx_indent(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut spaces = 0;
    while spaces < 3 && spaces < bytes.len() && bytes[spaces] == b' ' {
        spaces += 1;
    }
    if spaces == 3 && spaces < bytes.len() && bytes[spaces] == b' ' {
        return None;
    }
    Some(&line[spaces..])
}

fn atx_heading(line: &str) -> Option<(u8, String)> {
    let mut rest = line.trim_end_matches(['\n', '\r']);
    let mut spaces = 0;
    let bytes = rest.as_bytes();
    while spaces < 3 && spaces < bytes.len() && bytes[spaces] == b' ' {
        spaces += 1;
    }
    rest = &rest[spaces..];
    let hashes = rest.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &rest[hashes..];
    if after.is_empty() || !after.starts_with([' ', '\t']) {
        return None;
    }
    let mut title = after.trim().to_owned();
    while title.ends_with('#') {
        let stripped = title.trim_end_matches('#');
        if stripped.ends_with(' ') || stripped.ends_with('\t') || stripped.is_empty() {
            title = stripped.trim_end().to_owned();
        } else {
            break;
        }
    }
    Some((hashes as u8, title))
}

fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "section".to_owned()
    } else {
        out
    }
}

fn unique_key(slug: &str, used: &mut Vec<String>) -> String {
    if !used.iter().any(|k| k == slug) {
        used.push(slug.to_owned());
        return slug.to_owned();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{slug}-{n}");
        if !used.iter().any(|k| k == &candidate) {
            used.push(candidate.clone());
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHARS_PER_TOKEN, WHOLE_BODY_CHEAPER_TOKENS, estimate_tokens, outline_of, skill_section,
        slugify, split_sections,
    };

    const SAMPLE: &str = "\
Lead-in paragraph.

# Setup
Install the tool.

## Details
Nested steps.

# Setup
Second setup.

# Roles
On-call owns the page.
";

    #[test]
    fn estimate_tokens_is_four_chars() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(CHARS_PER_TOKEN, 4);
        assert_eq!(WHOLE_BODY_CHEAPER_TOKENS, 1000);
    }

    #[test]
    fn preamble_and_flat_parent_exclude_children() {
        let sections = split_sections(SAMPLE);
        let keys: Vec<&str> = sections.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["preamble", "setup", "details", "setup-2", "roles"]);
        assert_eq!(sections[0].title, "Preamble");
        assert_eq!(sections[0].level, 0);
        assert!(sections[0].body.contains("Lead-in"));
        assert!(!sections[1].body.contains("Nested steps"));
        assert!(sections[2].body.contains("Nested steps"));
        assert_eq!(sections[3].title, "Setup");
        assert!(sections[3].body.contains("Second setup"));
    }

    #[test]
    fn no_headings_is_preamble() {
        let sections = split_sections("just text\n");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key, "preamble");
        assert_eq!(sections[0].body, "just text");
    }

    #[test]
    fn empty_body_has_no_sections() {
        assert!(split_sections("").is_empty());
        assert!(split_sections("   \n").is_empty());
    }

    #[test]
    fn heading_preamble_collision_gets_dash_two() {
        let sections = split_sections("# Preamble\nbody\n");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key, "preamble");
        let both = split_sections("intro\n\n# Preamble\nbody\n");
        let keys: Vec<&str> = both.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["preamble", "preamble-2"]);
    }

    #[test]
    fn punctuation_heading_slug_is_section() {
        assert_eq!(slugify("!!!"), "section");
        assert_eq!(slugify("Set Up"), "set-up");
        assert_eq!(slugify("Rôles"), "rôles");
    }

    #[test]
    fn outline_marks_small_body_cheaper() {
        let outline = outline_of(SAMPLE);
        assert!(outline.whole_body_is_cheaper);
        assert_eq!(outline.sections.len(), 5);
        assert_eq!(outline.sections[1].key, "setup");
        assert!(outline.sections[1].tokens > 0);
    }

    #[test]
    fn outline_marks_large_body_not_cheaper() {
        let long = format!("# Big\n{}\n", "word ".repeat(1200));
        let outline = outline_of(&long);
        assert!(outline.whole_tokens >= WHOLE_BODY_CHEAPER_TOKENS);
        assert!(!outline.whole_body_is_cheaper);
    }

    #[test]
    fn skill_section_returns_body_or_lists_keys() {
        let setup = skill_section(SAMPLE, "setup").expect("setup");
        assert!(setup.body.starts_with("# Setup"));
        assert!(!setup.body.contains("Nested steps"));
        let err = skill_section(SAMPLE, "missing").expect_err("miss");
        assert!(err.contains("unknown section: missing"));
        assert!(err.contains("preamble, setup, details, setup-2, roles"));
        let hostile = skill_section(SAMPLE, "bad\nkey").expect_err("nl");
        assert!(!hostile.contains('\n'));
        assert!(hostile.contains("bad?key"));
    }

    #[test]
    fn atx_needs_space_and_strips_closing_hashes() {
        let hash_run = split_sections("#Not a heading\n# Real ##\n");
        assert_eq!(hash_run.len(), 2);
        assert_eq!(hash_run[0].key, "preamble");
        assert!(hash_run[0].body.contains("#Not a heading"));
        assert_eq!(hash_run[1].title, "Real");
        assert_eq!(hash_run[1].key, "real");
    }

    #[test]
    fn fenced_hash_comments_are_not_headings() {
        let body = "\
# Setup

Run the installer.

```bash
# Install deps
npm install
# Build
npm run build
```

More setup prose that belongs to Setup.

# Usage

Use it.
";
        let sections = split_sections(body);
        let keys: Vec<&str> = sections.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["setup", "usage"], "keys={keys:?}");
        assert!(
            sections[0].body.contains("More setup prose"),
            "parent must keep post-fence prose: {}",
            sections[0].body
        );
        assert!(
            sections[0].body.contains("# Install deps"),
            "fence body stays in Setup: {}",
            sections[0].body
        );
        let setup = skill_section(body, "setup").expect("setup");
        assert!(setup.body.contains("More setup prose"), "{}", setup.body);
        assert!(skill_section(body, "install-deps").is_err());
        assert!(skill_section(body, "build").is_err());
    }

    #[test]
    fn tilde_fence_and_backtick_close_do_not_cross() {
        let body = "\
# One

~~~
# Not a heading
```
# Still inside tildes
~~~

# Two
";
        let sections = split_sections(body);
        let keys: Vec<&str> = sections.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["one", "two"], "keys={keys:?}");
    }
}
