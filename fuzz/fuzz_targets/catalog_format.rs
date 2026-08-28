#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mid = data.len() / 2;
    let name = String::from_utf8_lossy(&data[..mid]);
    let desc = String::from_utf8_lossy(&data[mid..]);
    let mut first = craftbag::Skill::new(name.as_ref(), desc.as_ref(), text.as_ref());
    first.when_to_use = Some(text.as_ref().to_owned());
    let second = craftbag::Skill::new(desc.as_ref(), name.as_ref(), "x");
    let budgets = craftbag::ProgressiveBudgets {
        catalog_max_entries: 1,
        catalog_max_chars: 64_000,
        body_token_budget: 100,
    };
    let cat = craftbag::format_catalog(
        &[first, second],
        text.as_ref(),
        budgets,
        craftbag::FormatOptions::default(),
    );
    assert!(
        !cat.contains('\u{2028}') && !cat.contains('\u{2029}'),
        "catalog must flatten line/paragraph separators so list items stay one line"
    );
    if !cat.is_empty() {
        assert!(
            cat.starts_with("## Skills\n"),
            "catalog wrapper must stay intact"
        );
        assert!(
            cat.contains("Prefer a matching skill over improvising process."),
            "catalog must keep the host-neutral prefer line"
        );
        assert!(
            cat.contains("more skills not listed"),
            "two skills and max_entries=1 must omit the rest: {cat}"
        );
        for line in cat.lines() {
            if line.starts_with("- **") {
                assert!(
                    !line.contains('\r'),
                    "catalog skill item must stay one markdown line: {line:?}"
                );
            }
        }
    }

    let entries = usize::from(data.first().copied().unwrap_or(0)) % 5;
    let chars = usize::from(data.get(1).copied().unwrap_or(0)).saturating_add(1) * 16;
    let mut tight_skill = craftbag::Skill::new(text.as_ref(), text.as_ref(), "x");
    tight_skill.when_to_use = Some(text.as_ref().to_owned());
    let tight = craftbag::ProgressiveBudgets {
        catalog_max_entries: entries,
        catalog_max_chars: chars,
        body_token_budget: 1,
    };
    let cat = craftbag::format_catalog(
        &[tight_skill],
        "",
        tight,
        craftbag::FormatOptions::default(),
    );
    assert!(
        cat.len() <= chars,
        "tight catalog must stay within catalog_max_chars: len {} > {}",
        cat.len(),
        chars
    );
});
