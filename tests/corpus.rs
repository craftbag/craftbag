//! Committed corpus fixtures. Parse here; discovery uses the same files
//! from `src/discover.rs` unit tests (home override is crate-private).

use std::fs;
use std::path::PathBuf;

use craftbag::{ParseError, parse_skill};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn read(rel: &str) -> String {
    fs::read_to_string(corpus().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn corpus_minimal_valid_parses() {
    let skill = parse_skill(&read("agentskills/minimal-valid/SKILL.md")).expect("parse");
    assert_eq!(skill.name, "minimal-valid");
    assert!(skill.content.contains("Follow these steps."));
}

#[test]
fn corpus_invalid_name_is_parse_error() {
    let err = parse_skill(&read("agentskills/invalid-name/Bad_Name/SKILL.md")).unwrap_err();
    assert!(matches!(err, ParseError::InvalidYaml(_)));
}

#[test]
fn corpus_package_full_parses_license() {
    let skill = parse_skill(&read("agentskills/package-full/SKILL.md")).expect("parse");
    assert_eq!(skill.name, "package-full");
    assert_eq!(skill.license.as_deref(), Some("MIT"));
    assert!(
        corpus()
            .join("agentskills/package-full/scripts/hello.sh")
            .is_file()
    );
}

#[test]
fn corpus_incumbent_cursor_project_parses() {
    let rel = "incumbent/cursor-project/.cursor/skills/create-rule/SKILL.md";
    assert!(
        corpus().join(rel).is_file(),
        "committed Cursor project fixture must exist: {rel}"
    );
    let skill = parse_skill(&read(rel)).expect("parse");
    assert_eq!(skill.name, "create-rule");
}

#[test]
fn corpus_incumbent_cursor_user_parses() {
    let rel = "incumbent/cursor-user/.cursor/skills/home-rule/SKILL.md";
    assert!(
        corpus().join(rel).is_file(),
        "committed Cursor user-home fixture must exist: {rel}"
    );
    let skill = parse_skill(&read(rel)).expect("parse");
    assert_eq!(skill.name, "home-rule");
}
