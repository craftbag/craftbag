#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    match craftbag::parse_list_format(&text) {
        Ok(_) => {
            let token = text.trim();
            assert!(
                craftbag::ListFormat::CANONICAL_TOKENS.contains(&token)
                    || craftbag::ListFormat::ALIAS_TOKENS.contains(&token),
                "only table tokens parse: {text:?}"
            );
        }
        Err(msg) => {
            assert!(
                !msg.contains('\u{2014}')
                    && !msg.contains('\u{2028}')
                    && !msg.contains('\u{2029}')
                    && !msg.contains('\n')
                    && !msg.contains('\r')
                    && !msg.contains('\0'),
                "format errors must stay one line without an em dash: {msg:?}"
            );
            assert!(
                msg.contains("unknown format:"),
                "parse errors must name the miss: {msg}"
            );
            if text.trim().is_empty() {
                assert!(
                    msg.contains("empty"),
                    "whitespace-only format must say empty: {msg}"
                );
            }
        }
    }
    let hint = craftbag::unknown_list_format(&text);
    assert!(
        !hint.contains('\u{2014}')
            && !hint.contains('\u{2028}')
            && !hint.contains('\u{2029}')
            && !hint.contains('\n')
            && !hint.contains('\r')
            && !hint.contains('\0'),
        "unknown_list_format must stay one line without an em dash: {hint:?}"
    );
});
