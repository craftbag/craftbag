//! Public types, SKILL.md parse, and multi-root discovery.

mod discover;
mod error;
mod parse;
mod skill;
mod skip;
mod source;

pub use discover::{CURSOR_VENDOR_DENYLIST, DiscoveryOptions, discover, find_skill_by_name};
pub use error::{Error, ParseError};
pub use parse::{parse_skill, skill_name_matches_directory, validate_skill_name};
pub use skill::{
    SKILL_BODY_LINE_SOFT_WARN, SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS,
    SKILL_NAME_MAX_CHARS, Skill,
};
pub use skip::{DiscoveryReport, SkillSkip, SkipKind};
pub use source::SkillSource;

/// Package version from `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_nonzero() {
        assert!(!super::version().is_empty());
    }
}
