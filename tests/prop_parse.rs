//! Property tests for `parse_skill` (name charset, description length, missing ---,
//! present-null invocation flags).

use craftbag::{
    ParseError, SKILL_COMPATIBILITY_MAX_CHARS, SKILL_DESCRIPTION_MAX_CHARS, SKILL_NAME_MAX_CHARS,
    normalize_skill_name, parse_skill, validate_skill_name,
};
use proptest::prelude::*;

fn valid_name() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z0-9]{1,8}", 1usize..6)
        .prop_map(|parts| parts.join("-"))
        .prop_filter("name length", |s| {
            let n = s.chars().count();
            (1..=64).contains(&n)
        })
}

/// Scalars that `parse_bool_yaml` must not accept.
///
/// Unit fixtures lock `null` / `~` / empty / `maybe`. This generator
/// also covers other words and digits so a silent omitted default
/// cannot return for an unlisted token. Snake keys only; hyphen
/// aliases stay in the unit table.
fn non_bool_yaml_scalar() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("~".to_owned()),
        Just("null".to_owned()),
        "[2-9]{1,6}",
        "[a-z]{2,12}".prop_filter("not a yaml bool word", |s| {
            !matches!(s.as_str(), "true" | "false" | "yes" | "no" | "on" | "off")
        }),
    ]
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
        prop_assume!(!body
            .trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
            .starts_with("---"));
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
    fn compatibility_over_max_is_rejected(over in 1usize..16) {
        let compat = "c".repeat(SKILL_COMPATIBILITY_MAX_CHARS + over);
        let md = format!("---\nname: ok-name\ndescription: d\ncompatibility: {compat}\n---\n");
        let err = parse_skill(&md).unwrap_err();
        prop_assert!(matches!(err, ParseError::InvalidYaml(_)));
    }

    #[test]
    fn name_over_max_is_rejected(over in 1usize..16) {
        let name = "a".repeat(SKILL_NAME_MAX_CHARS + over);
        prop_assert!(validate_skill_name(&name).is_err());
        let md = format!("---\nname: {name}\ndescription: d\n---\n");
        prop_assert!(parse_skill(&md).is_err());
    }

    #[test]
    fn parse_skill_does_not_panic(s in ".{0,200}") {
        let _ = parse_skill(&s);
    }

    #[test]
    fn unicode_lowercase_names_parse(
        chunks in prop::collection::vec(unicode_name_chunk(), 1usize..4)
    ) {
        let name = chunks.join("-");
        prop_assume!(validate_skill_name(&name).is_ok());
        let md = format!("---\nname: {name}\ndescription: d\n---\nbody\n");
        let skill = parse_skill(&md).expect("valid unicode name");
        prop_assert_eq!(skill.name, normalize_skill_name(&name));
        prop_assert_eq!(skill.description, "d");
    }

    #[test]
    fn cyrillic_uppercase_name_is_rejected(tail in unicode_name_chunk()) {
        let name = format!("Я{tail}");
        prop_assert!(validate_skill_name(&name).is_err());
        let md = format!("---\nname: {name}\ndescription: d\n---\n");
        prop_assert!(parse_skill(&md).is_err());
    }

    #[test]
    fn present_non_bool_invocation_flag_is_error(
        raw in non_bool_yaml_scalar(),
        user_invocable in any::<bool>(),
    ) {
        let key = if user_invocable {
            "user_invocable"
        } else {
            "disable_model_invocation"
        };
        let md = format!("---\nname: ok-name\ndescription: d\n{key}: {raw}\n---\nbody\n");
        let err = parse_skill(&md).unwrap_err();
        prop_assert!(
            matches!(err, ParseError::InvalidYaml(ref m) if m.contains(key)),
            "{key}: {raw:?} -> {err}"
        );
    }
}

fn unicode_name_chunk() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            prop::char::range('a', 'z'),
            prop::char::range('а', 'я'),
            prop::sample::select(vec!['ё', 'ü', 'ö', 'é', 'ñ', '中', '文']),
        ],
        1usize..6,
    )
    .prop_map(|cs| cs.into_iter().collect())
}
