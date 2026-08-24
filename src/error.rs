//! Parse and crate errors. Display strings match Bline `SkillParseError`.

/// Frontmatter or agentskills field failure.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The file does not contain YAML frontmatter delimiters (`---`).
    #[error("missing YAML frontmatter")]
    MissingFrontmatter,
    /// A required frontmatter field is absent.
    #[error("missing required field: {0}")]
    MissingField(String),
    /// The YAML frontmatter could not be parsed.
    #[error("invalid YAML: {0}")]
    InvalidYaml(String),
}

/// Crate-level error. Discovery IO variants land with the discovery slice.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum Error {
    /// Frontmatter or agentskills validation failed.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

#[cfg(test)]
mod tests {
    use super::{Error, ParseError};

    #[test]
    fn parse_error_display_matches_bline() {
        assert_eq!(
            ParseError::MissingFrontmatter.to_string(),
            "missing YAML frontmatter"
        );
        assert_eq!(
            ParseError::MissingField("name".to_owned()).to_string(),
            "missing required field: name"
        );
        assert_eq!(
            ParseError::InvalidYaml("bad indent".to_owned()).to_string(),
            "invalid YAML: bad indent"
        );
    }

    #[test]
    fn error_wraps_parse_error() {
        let err: Error = ParseError::MissingFrontmatter.into();
        assert_eq!(err.to_string(), "missing YAML frontmatter");
        assert!(matches!(err, Error::Parse(ParseError::MissingFrontmatter)));
    }
}
