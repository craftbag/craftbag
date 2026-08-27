#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

fn leftover_rows_stay_one_line(out: &str, rows: usize) {
    assert_eq!(
        out.chars().filter(|&c| c == '\n').count(),
        rows,
        "leftover path must not add extra lines: {out:?}"
    );
    for line in out.lines() {
        assert!(
            !line.contains('\u{2014}')
                && !line.contains('\u{2028}')
                && !line.contains('\u{2029}')
                && !line.contains('\r')
                && !line.contains('\0'),
            "leftover path row must stay one line without an em dash: {line:?}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let path = PathBuf::from(text.as_ref());

    leftover_rows_stay_one_line(&craftbag::format_watch_dirs(&[path.clone()]), 1);

    let mut skill = craftbag::Skill::new("demo", "d", "");
    skill.source_path = Some(path.clone());
    leftover_rows_stay_one_line(&craftbag::format_list_tsv(&[skill.clone()]), 1);

    let why = craftbag::format_why_text(&craftbag::WhyReport {
        loaded: vec![craftbag::SkillSummary::from(&skill)],
        skips: vec![craftbag::SkillSkip {
            path,
            name: None,
            kind: craftbag::SkipKind::Unreadable,
            detail: text.as_ref().to_owned(),
            winner_path: None,
        }],
        activation: vec![craftbag::ActivationDecision {
            name: "demo".to_owned(),
            reason: craftbag::ActivationReason::Injected,
            detail: text.as_ref().to_owned(),
        }],
        query: None,
    });
    leftover_rows_stay_one_line(&why, 3);
});
