//! Activation selector: budgets, trigger filter, catalog, and package envelope.

use std::path::Path;

use crate::skill::{SKILL_BODY_LINE_SOFT_WARN, Skill};
use crate::source::SkillSource;

/// Host-neutral wording for how to load one full skill body.
pub const DEFAULT_ACTIVATE_HINT: &str =
    "Use the host activate command to load full instructions for one skill.";

/// Catalog and auto-body budgets derived from the model context window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressiveBudgets {
    /// Max catalog lines (name + description).
    pub catalog_max_entries: usize,
    /// Max characters for the catalog fragment (including header).
    pub catalog_max_chars: usize,
    /// Token budget for auto-injected full skill bodies (`content.len()/4`).
    pub body_token_budget: usize,
}

/// Host-supplied strings used when formatting catalog and load text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions<'a> {
    /// How the host tells the model to load one skill.
    pub activate_hint: &'a str,
}

impl Default for FormatOptions<'static> {
    fn default() -> Self {
        Self {
            activate_hint: DEFAULT_ACTIVATE_HINT,
        }
    }
}

/// Derive catalog + body budgets from the model context window size.
///
/// `context_tokens` is the model's full context window (not free space).
pub fn progressive_budgets(context_tokens: usize) -> ProgressiveBudgets {
    let ctx = context_tokens.max(4_000);
    let catalog_token_budget = (ctx / 100).clamp(250, 25_000);
    let catalog_max_chars = catalog_token_budget.saturating_mul(4);
    let catalog_max_entries = (catalog_token_budget / 20).clamp(8, 1_000);
    let body_token_budget = ctx
        .saturating_mul(2)
        .saturating_div(100)
        .clamp(300, 100_000);
    ProgressiveBudgets {
        catalog_max_entries,
        catalog_max_chars,
        body_token_budget,
    }
}

/// Case-insensitive trigger match on word/token boundaries, not substrings.
///
/// `context_lower` must already be lowercased. Empty triggers never match.
pub fn trigger_matches(context_lower: &str, trigger: &str) -> bool {
    let needle = trigger.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let hay = context_lower;
    let mut search_from = 0;
    while search_from <= hay.len() {
        let Some(rel) = hay[search_from..].find(&needle) else {
            return false;
        };
        let start = search_from + rel;
        let end = start + needle.len();
        if !hay.is_char_boundary(end) {
            return false;
        }
        let before_ok = start == 0
            || hay[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !is_trigger_word_char(c));
        let after_ok = end == hay.len()
            || hay[end..]
                .chars()
                .next()
                .is_none_or(|c| !is_trigger_word_char(c));
        if before_ok && after_ok {
            return true;
        }
        let Some(ch) = hay[start..].chars().next() else {
            return false;
        };
        search_from = start + ch.len_utf8();
    }
    false
}

fn is_trigger_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_vendor_compat(source: &SkillSource) -> bool {
    match source {
        SkillSource::Vendor { name } => matches!(name.as_str(), "claude" | "cursor" | "grok"),
        _ => false,
    }
}

/// Filter skills by trigger match and token budget.
///
/// Vendor sources `claude` / `cursor` / `grok` with empty triggers are not
/// always-active. `disable_model_invocation` never auto-injects.
pub fn filter_skills<'a>(
    skills: &'a [Skill],
    context: &str,
    token_budget: usize,
) -> Vec<&'a Skill> {
    let context_lower = context.to_lowercase();
    let mut matched: Vec<&Skill> = Vec::new();
    let mut always_active: Vec<&Skill> = Vec::new();

    for skill in skills {
        if skill.disable_model_invocation {
            continue;
        }
        if skill.triggers.is_empty() {
            if !is_vendor_compat(&skill.source) {
                always_active.push(skill);
            }
        } else {
            let has_match = skill
                .triggers
                .iter()
                .any(|t| trigger_matches(&context_lower, t));
            if has_match {
                matched.push(skill);
            }
        }
    }

    matched.sort_by(|a, b| {
        skill_relevance_score(b, &context_lower)
            .cmp(&skill_relevance_score(a, &context_lower))
            .then_with(|| a.name.cmp(&b.name))
    });
    always_active.sort_by(|a, b| a.name.cmp(&b.name));

    let mut result = Vec::new();
    let mut used_tokens: usize = 0;

    for skill in matched.into_iter().chain(always_active) {
        let envelope_overhead = if skill.package_root().is_some() {
            120
        } else {
            0
        };
        let estimated_tokens = skill.content.len() / 4 + envelope_overhead;
        if used_tokens.saturating_add(estimated_tokens) > token_budget && !result.is_empty() {
            continue;
        }
        used_tokens = used_tokens.saturating_add(estimated_tokens.min(token_budget));
        result.push(skill);
    }

    result
}

/// Relevance score for ranking skills against user text.
pub fn skill_relevance_score(skill: &Skill, context_lower: &str) -> i32 {
    if context_lower.is_empty() {
        return 0;
    }
    let mut score: i32 = 0;
    for t in &skill.triggers {
        if trigger_matches(context_lower, t) {
            score = score.saturating_add(100);
        }
    }
    let name_l = skill.name.to_lowercase();
    if context_lower.contains(&name_l) {
        score = score.saturating_add(50);
    }
    let name_words = name_l.replace('-', " ");
    if name_words != name_l && context_lower.contains(&name_words) {
        score = score.saturating_add(40);
    }
    for text in [
        skill.description.as_str(),
        skill.when_to_use.as_deref().unwrap_or(""),
    ] {
        for word in text.split_whitespace() {
            let w: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
                .to_lowercase();
            if w.len() >= 4 && context_lower.contains(&w) {
                score = score.saturating_add(1);
            }
        }
    }
    score
}

/// Rank skills for catalog display: high relevance first, then name.
pub fn rank_skills_for_catalog<'a>(skills: &'a [Skill], context: &str) -> Vec<&'a Skill> {
    let context_lower = context.to_lowercase();
    let mut ranked: Vec<&Skill> = skills.iter().collect();
    ranked.sort_by(|a, b| {
        skill_relevance_score(b, &context_lower)
            .cmp(&skill_relevance_score(a, &context_lower))
            .then_with(|| a.name.cmp(&b.name))
    });
    ranked
}

/// Cap a skill body to roughly `token_budget` tokens (`chars ≈ tokens * 4`).
pub fn truncate_skill_body_for_budget(content: &str, token_budget: usize) -> String {
    if token_budget == 0 {
        return String::new();
    }
    let max_chars = token_budget.saturating_mul(4);
    if content.len() <= max_chars {
        return content.to_owned();
    }
    let mut cut = max_chars.saturating_sub(80);
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = content[..cut].to_owned();
    out.push_str(
        "\n\n…(skill body truncated for small context; use the host activate command for the full text)\n",
    );
    out
}

/// Build a cheap catalog fragment: name + description only.
pub fn format_catalog(
    skills: &[Skill],
    context: &str,
    budgets: ProgressiveBudgets,
    fmt: FormatOptions<'_>,
) -> String {
    if skills.is_empty() || budgets.catalog_max_entries == 0 || budgets.catalog_max_chars == 0 {
        return String::new();
    }

    let ranked = rank_skills_for_catalog(skills, context);
    let header = format!(
        "## Skills\n{}\nPrefer a matching skill over improvising process.\n\n",
        fmt.activate_hint
    );

    let mut body = String::new();
    let mut shown = 0usize;
    let mut omitted = 0usize;
    let ranked_len = ranked.len();

    for skill in &ranked {
        let line = format!("- **{}**: {}\n", skill.name, skill.description);
        if shown >= budgets.catalog_max_entries {
            omitted = omitted.saturating_add(1);
            continue;
        }
        let reserve = 80usize;
        let limit = budgets.catalog_max_chars.saturating_sub(reserve);
        if header
            .len()
            .saturating_add(body.len())
            .saturating_add(line.len())
            > limit
            && shown > 0
        {
            omitted = omitted.saturating_add(1 + (ranked_len.saturating_sub(shown + 1)));
            break;
        }
        if header.len().saturating_add(line.len()) > limit && shown == 0 {
            let room = limit.saturating_sub(header.len()).saturating_sub(8);
            if room < 16 {
                break;
            }
            let mut short = line;
            truncate_at_char_boundary(&mut short, room);
            short.push('…');
            short.push('\n');
            body.push_str(&short);
            shown = 1;
            omitted = ranked_len.saturating_sub(1);
            break;
        }
        body.push_str(&line);
        shown = shown.saturating_add(1);
    }

    if shown == 0 {
        return String::new();
    }

    let mut out = String::with_capacity(header.len() + body.len() + 96);
    out.push_str(&header);
    out.push_str(&body);
    if omitted > 0 {
        out.push_str(&format!(
            "\n(…{omitted} more skills not listed; {hint})\n",
            hint = fmt.activate_hint
        ));
    }
    if out.len() > budgets.catalog_max_chars {
        truncate_at_char_boundary(&mut out, budgets.catalog_max_chars.saturating_sub(1));
        out.push('…');
    }
    out
}

/// Official skills-ref `<available_skills>` XML for host system prompts.
pub fn format_available_skills_xml(skills: &[Skill]) -> String {
    let mut out = String::from("<available_skills>\n");
    for skill in skills {
        let location = skill
            .source_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        out.push_str("<skill>\n");
        out.push_str("<name>");
        out.push_str(&xml_escape(&skill.name));
        out.push_str("</name>\n");
        out.push_str("<description>");
        out.push_str(&xml_escape(&skill.description));
        out.push_str("</description>\n");
        out.push_str("<location>");
        out.push_str(&xml_escape(&location));
        out.push_str("</location>\n");
        out.push_str("</skill>\n");
    }
    out.push_str("</available_skills>\n");
    out
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if is_xml10_char(c) => out.push(c),
            // XML 1.0 cannot represent these even as character references.
            _ => {}
        }
    }
    out
}

/// XML 1.0 `Char` production (https://www.w3.org/TR/xml/#charsets).
fn is_xml10_char(c: char) -> bool {
    matches!(
        c,
        '\t'
            | '\n'
            | '\r'
            | '\u{20}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}'
            | '\u{10000}'..='\u{10FFFF}'
    )
}

fn truncate_at_char_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

/// Skill package root plus capped listings of scripts/references/assets.
///
/// Does not inline file contents.
pub fn format_package_envelope(skill: &Skill) -> String {
    let mut out = String::new();
    let Some(root) = skill.package_root() else {
        out.push_str(
            "Skill package root: (unknown; SKILL.md path not set. Relative scripts/ paths may resolve incorrectly)\n",
        );
        return out;
    };
    out.push_str(&format!("Skill package root: {}\n", root.display()));
    out.push_str(
        "Relative paths in this skill (scripts/…, references/…, assets/…) are relative to the skill package root, not the project cwd.\n",
    );
    out.push_str("Optional package dirs (load on demand; prefer absolute paths):\n");
    let root_canon = root.canonicalize().ok();
    for dir_name in ["scripts", "references", "assets"] {
        let dir = root.join(dir_name);
        if !dir.is_dir() {
            out.push_str(&format!("  {dir_name}/: (not present)\n"));
            continue;
        }
        if package_dir_escapes(root_canon.as_deref(), &dir) {
            out.push_str(&format!(
                "  {dir_name}/: (skipped: package dir escapes skill root)\n"
            ));
            continue;
        }
        let listing = list_dir_names_capped(&dir, 30);
        if listing.is_empty() {
            out.push_str(&format!("  {dir_name}/: {} (empty)\n", dir.display()));
        } else {
            out.push_str(&format!(
                "  {dir_name}/: {} (files: {})\n",
                dir.display(),
                listing.join(", ")
            ));
        }
    }
    out
}

fn package_dir_escapes(root_canon: Option<&Path>, dir: &Path) -> bool {
    let Some(root_c) = root_canon else {
        return false;
    };
    match dir.canonicalize() {
        Ok(dir_c) => !dir_c.starts_with(root_c),
        Err(_) => false,
    }
}

fn list_dir_names_capped(dir: &Path, max: usize) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    if names.len() > max {
        let extra = names.len() - max;
        names.truncate(max);
        names.push(format!("…+{extra} more"));
    }
    names
}

/// Header for auto-injected skill bodies.
pub fn format_body_header(skill: &Skill) -> String {
    let mut out = format!("### Skill: {}\n{}\n\n", skill.name, skill.description);
    out.push_str(&format_package_envelope(skill));
    out.push('\n');
    out
}

/// User-turn payload that asks the model to follow one skill fully.
pub fn format_load_message(skill: &Skill, arguments: &str, fmt: FormatOptions<'_>) -> String {
    let args = arguments.trim();
    let mut out = String::new();
    out.push_str(&format!("[Activated skill: {}]\n\n", skill.name));
    out.push_str("Follow this skill completely for the rest of this turn.\n");
    if !skill.description.is_empty() {
        out.push_str(&format!("Description: {}\n", skill.description));
    }
    if let Some(lic) = &skill.license {
        out.push_str(&format!("License: {lic}\n"));
    }
    if let Some(compat) = &skill.compatibility {
        out.push_str(&format!("Compatibility: {compat}\n"));
    }
    if !args.is_empty() {
        out.push_str(&format!("User arguments: {args}\n"));
    }
    let body_lines = skill.content.lines().count();
    if body_lines > SKILL_BODY_LINE_SOFT_WARN {
        out.push_str(&format!(
            "Note: this SKILL.md has {body_lines} lines (agentskills recommends ~500). Prefer splitting detail into references/ under the skill package root.\n"
        ));
    }
    out.push_str(&format!("Activate hint: {}\n", fmt.activate_hint));
    out.push('\n');
    out.push_str(&format_package_envelope(skill));
    out.push('\n');
    out.push_str("---\n");
    out.push_str(skill.content.trim());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_ACTIVATE_HINT, FormatOptions, ProgressiveBudgets, filter_skills,
        format_available_skills_xml, format_catalog, format_load_message, format_package_envelope,
        progressive_budgets, trigger_matches, truncate_skill_body_for_budget,
    };
    use crate::parse::parse_skill;
    use crate::skill::Skill;
    use crate::source::SkillSource;
    use std::path::PathBuf;

    fn make_skill(name: &str, triggers: &[&str], content_len: usize) -> Skill {
        let mut s = Skill::new(name, format!("{name} description"), "x".repeat(content_len));
        s.triggers = triggers.iter().map(|&t| t.to_owned()).collect();
        s
    }

    #[test]
    fn progressive_budgets_scale_with_context() {
        let small = progressive_budgets(8_000);
        let mid = progressive_budgets(32_000);
        let large = progressive_budgets(128_000);
        let huge = progressive_budgets(500_000);
        assert!(small.catalog_max_entries < mid.catalog_max_entries);
        assert!(mid.catalog_max_entries < large.catalog_max_entries);
        assert!(large.catalog_max_entries < huge.catalog_max_entries);
        assert!(small.body_token_budget < mid.body_token_budget);
        assert!(mid.body_token_budget < large.body_token_budget);
        assert!(large.body_token_budget < huge.body_token_budget);
        assert_eq!(huge.body_token_budget, 10_000);
        assert_eq!(huge.catalog_max_entries, 250);
        let tiny = progressive_budgets(0);
        assert_eq!(tiny.body_token_budget, 300);
        assert_eq!(tiny.catalog_max_entries, 12);
    }

    #[test]
    fn progressive_budgets_max_context_does_not_overflow() {
        let huge = progressive_budgets(usize::MAX);
        assert_eq!(huge.body_token_budget, 100_000);
        assert_eq!(huge.catalog_max_entries, 1_000);
        assert_eq!(huge.catalog_max_chars, 100_000);
    }

    #[test]
    fn filter_skills_matching_trigger() {
        let skills = vec![
            make_skill("git-workflow", &["git", "commit"], 100),
            make_skill("rust-style", &["rust", "cargo"], 100),
        ];
        let result = filter_skills(&skills, "working with git rebase", 10000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "git-workflow");
    }

    #[test]
    fn filter_skills_no_triggers_always_active() {
        let skills = vec![
            make_skill("always-on", &[], 100),
            make_skill("conditional", &["special"], 100),
        ];
        let result = filter_skills(&skills, "normal context", 10000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "always-on");
    }

    #[test]
    fn filter_skills_vendor_empty_triggers_not_always_active() {
        let mut vendor = make_skill("personal", &[], 100);
        vendor.source = SkillSource::Vendor {
            name: "grok".to_owned(),
        };
        let skills = vec![vendor];
        assert!(filter_skills(&skills, "hello", 10_000).is_empty());
    }

    #[test]
    fn filter_skills_bline_vendor_empty_triggers_is_always_active() {
        let mut vendor = make_skill("legacy", &[], 100);
        vendor.source = SkillSource::Vendor {
            name: "bline".to_owned(),
        };
        let skills = vec![vendor];
        assert_eq!(filter_skills(&skills, "hello", 10_000).len(), 1);
    }

    #[test]
    fn filter_skills_disable_model_invocation() {
        let mut s = make_skill("slash-only", &[], 100);
        s.disable_model_invocation = true;
        assert!(filter_skills(&[s], "anything", 10_000).is_empty());
    }

    #[test]
    fn filter_skills_budget_skips_huge_and_keeps_later_small() {
        let skills = vec![
            make_skill("a-small", &[], 80),
            make_skill("b-huge", &[], 2_000),
            make_skill("c-small", &[], 80),
        ];
        let result = filter_skills(&skills, "", 100);
        let names: Vec<&str> = result.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a-small"), "{names:?}");
        assert!(!names.contains(&"b-huge"), "{names:?}");
        assert!(names.contains(&"c-small"), "{names:?}");
    }

    #[test]
    fn filter_skills_short_trigger_does_not_match_substring() {
        let go = vec![make_skill("go-style", &["go"], 100)];
        assert!(filter_skills(&go, "I am going to the store", 10_000).is_empty());
    }

    #[test]
    fn filter_skills_exact_token_still_fires() {
        let go = vec![make_skill("go-style", &["go"], 100)];
        assert_eq!(
            filter_skills(&go, "please review this go module", 10_000).len(),
            1
        );
        assert_eq!(filter_skills(&go, "write Go, then ship", 10_000).len(), 1);
    }

    #[test]
    fn filter_skills_matched_before_always_active() {
        let skills = vec![
            make_skill("z-always", &[], 100),
            make_skill("a-triggered", &["rust"], 100),
        ];
        let result = filter_skills(&skills, "writing rust code", 10000);
        assert_eq!(result[0].name, "a-triggered");
        assert_eq!(result[1].name, "z-always");
    }

    #[test]
    fn trigger_matches_empty_is_false() {
        assert!(!trigger_matches("hello", "  "));
    }

    #[test]
    fn format_catalog_uses_host_hint_not_bline_slash() {
        let skills = vec![
            make_skill("git-workflow", &["git"], 10),
            make_skill("aaa-always", &[], 10),
        ];
        let budgets = ProgressiveBudgets {
            catalog_max_entries: 2,
            catalog_max_chars: 4_000,
            body_token_budget: 100,
        };
        let cat = format_catalog(
            &skills,
            "please help with git rebase",
            budgets,
            FormatOptions::default(),
        );
        assert!(cat.contains("git-workflow"), "{cat}");
        assert!(cat.contains(DEFAULT_ACTIVATE_HINT), "{cat}");
        assert!(!cat.contains("/skill"), "{cat}");
        assert!(!cat.contains("Available skills"), "{cat}");
    }

    #[test]
    fn format_available_skills_xml_escapes_and_lists_location() {
        let mut skill = make_skill("ampersand", &[], 10);
        skill.description = "A & B <tag>".to_owned();
        skill.source_path = Some(PathBuf::from("/tmp/ampersand/SKILL.md"));
        let xml = format_available_skills_xml(&[skill]);
        assert!(xml.starts_with("<available_skills>\n"), "{xml}");
        assert!(xml.contains("<name>ampersand</name>"), "{xml}");
        assert!(
            xml.contains("<description>A &amp; B &lt;tag&gt;</description>"),
            "{xml}"
        );
        assert!(
            xml.contains("<location>/tmp/ampersand/SKILL.md</location>"),
            "{xml}"
        );
        assert!(xml.ends_with("</available_skills>\n"), "{xml}");
    }

    #[test]
    fn format_available_skills_xml_strips_invalid_xml_chars() {
        let mut skill = make_skill("ctrl", &[], 10);
        skill.description = "ok\u{0000}bad\u{0001}\u{0008}\u{000B}\u{000C}\u{000E}".to_owned();
        skill.source_path = Some(PathBuf::from("/tmp/ctrl\u{0000}/SKILL.md"));
        let xml = format_available_skills_xml(&[skill]);
        assert!(
            !xml.chars().any(|c| !super::is_xml10_char(c)),
            "catalog must be XML 1.0: {xml:?}"
        );
        assert!(
            xml.contains("<description>okbad</description>"),
            "illegal controls are dropped, text remains: {xml}"
        );
        assert!(
            xml.contains("<location>/tmp/ctrl/SKILL.md</location>"),
            "location must drop NUL: {xml}"
        );
        assert!(xml.ends_with("</available_skills>\n"), "{xml}");
    }

    #[test]
    fn truncate_skill_body_for_budget_cuts_and_stays_host_neutral() {
        let body = "word ".repeat(200);
        let out = truncate_skill_body_for_budget(&body, 10);
        assert!(out.len() < body.len());
        assert!(out.contains("host activate command"));
        assert!(!out.contains("/skill"));
    }

    #[test]
    fn format_package_envelope_lists_package_full() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus/agentskills/package-full/SKILL.md");
        let text = std::fs::read_to_string(&path).expect("read");
        let mut skill = parse_skill(&text).expect("parse");
        skill.source_path = Some(path);
        let env = format_package_envelope(&skill);
        assert!(env.contains("hello.sh"), "{env}");
        assert!(env.contains("REF.md"), "{env}");
        assert!(env.contains("template.txt"), "{env}");
        assert!(!env.contains("#!/bin/sh"), "{env}");
        let load = format_load_message(&skill, "", FormatOptions::default());
        assert!(load.contains("[Activated skill: package-full]"));
        assert!(load.contains(DEFAULT_ACTIVATE_HINT));
        assert!(!load.contains("#!/bin/sh"));
    }

    #[test]
    fn format_package_envelope_unknown_root_has_no_em_dash() {
        let skill = Skill::new("x", "d", "body");
        let env = format_package_envelope(&skill);
        assert!(env.contains("unknown"));
        assert!(!env.contains('\u{2014}'));
    }
}
