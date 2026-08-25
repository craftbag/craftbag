#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

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

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut skill = craftbag::Skill::new(text.as_ref(), text.as_ref(), text.as_ref());
    skill.source_path = Some(PathBuf::from(text.as_ref()));
    skill.user_invocable = data.first().is_some_and(|b| b & 1 == 0);
    skill.disable_model_invocation = data.get(1).is_some_and(|b| b & 1 == 1);
    let xml = craftbag::format_available_skills_xml(&[skill]);
    assert!(
        xml.chars().all(is_xml10_char),
        "catalog XML must stay XML 1.0 after arbitrary name/description/path"
    );
    assert!(
        xml.starts_with("<available_skills>\n") && xml.ends_with("</available_skills>\n"),
        "catalog wrapper must stay intact"
    );
});
