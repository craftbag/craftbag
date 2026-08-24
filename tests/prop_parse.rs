//! Property tests for `parse_skill` (name charset, description length, missing ---).

use craftbag::{ParseError, SKILL_DESCRIPTION_MAX_CHARS, parse_skill, validate_skill_name};
use proptest::prelude::*;

fn valid_name() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z0-9]{1,8}", 1usize..6)
        .prop_map(|parts| parts.join("-"))
        .prop_filter("name length", |s| {
            let n = s.chars().count();
            (1..=64).contains(&n)
        })
}

proptest! {
    #[test]
    fn valid_names_parse(name in valid_name()) {
        prop_assert!(validate_skill_name(&name).is_ok());
        let md = format!("---\nname: {name}\ndescription: d\n---\nbody\n");
        let skill = parse_skill(&md).expect("valid name");
        prop_assert_eq!(skill.name, name);
        prop_assert_eq!(skill.description, "d");
    }

    #[test]
    fn missing_frontmatter_is_error(body in ".{0,80}") {
        prop_assume!(!body.trim_start().starts_with("---"));
        let err = parse_skill(&body).unwrap_err();
        prop_assert_eq!(err, ParseError::MissingFrontmatter);
    }

    #[test]
    fn description_over_max_is_error(over in 1usize..32) {
        let desc = "x".repeat(SKILL_DESCRIPTION_MAX_CHARS + over);
        let md = format!("---\nname: ok-name\ndescription: {desc}\n---\n");
        let err = parse_skill(&md).unwrap_err();
        prop_assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn uppercase_name_is_rejected(tail in "[a-z0-9]{1,8}") {
        let name = format!("A{tail}");
        prop_assert!(validate_skill_name(&name).is_err());
        let md = format!("---\nname: {name}\ndescription: d\n---\n");
        prop_assert!(parse_skill(&md).is_err());
    }

    #[test]
    fn consecutive_hyphens_are_rejected(left in "[a-z0-9]{1,6}", right in "[a-z0-9]{1,6}") {
        let name = format!("{left}--{right}");
        prop_assert!(validate_skill_name(&name).is_err());
        let md = format!("---\nname: {name}\ndescription: d\n---\n");
        prop_assert!(parse_skill(&md).is_err());
    }

    #[test]
    fn leading_or_trailing_hyphen_is_rejected(
        core in "[a-z0-9]{1,8}",
        lead in any::<bool>(),
    ) {
        let name = if lead {
            format!("-{core}")
        } else {
            format!("{core}-")
        };
        prop_assert!(validate_skill_name(&name).is_err());
        let md = format!("---\nname: {name}\ndescription: d\n---\n");
        prop_assert!(parse_skill(&md).is_err());
    }

    #[test]
    fn parse_skill_does_not_panic(s in ".{0,200}") {
        let _ = parse_skill(&s);
    }
}
