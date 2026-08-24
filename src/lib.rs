//! Public types for skill packages and the frozen skip-kind taxonomy.

mod error;
mod skill;
mod skip;
mod source;

pub use error::{Error, ParseError};
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
