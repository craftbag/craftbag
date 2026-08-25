//! Parse and crate errors. Display strings match Bline `SkillParseError`.

/// Echo a host or CLI token on one stderr line.
///
/// Control characters become `?`. U+2028 / U+2029 (line and
/// paragraph separators) also become `?`; they are not `Cc` so
/// `is_control` misses them. U+2014 becomes ASCII `-`.
pub fn sanitize_error_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\u{2014}' => out.push('-'),
            '\u{2028}' | '\u{2029}' => out.push('?'),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

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
    use super::{Error, ParseError, sanitize_error_token};

    #[test]
    fn sanitize_error_token_keeps_one_line() {
        assert_eq!(sanitize_error_token("json\nxml"), "json?xml");
        assert_eq!(sanitize_error_token("foo\0bar"), "foo?bar");
        assert_eq!(sanitize_error_token("foo\u{2014}bar"), "foo-bar");
        assert_eq!(sanitize_error_token("   "), "   ");
        assert_eq!(
            sanitize_error_token("json\u{2028}xml"),
            "json?xml",
            "U+2028 must not split a CLI or MCP error line"
        );
        assert_eq!(
            sanitize_error_token("json\u{2029}xml"),
            "json?xml",
            "U+2029 must not split a CLI or MCP error line"
        );
    }

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
