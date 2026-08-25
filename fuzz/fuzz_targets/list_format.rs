#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    match craftbag::parse_list_format(&text) {
        Ok(_) => {
            assert!(
                matches!(text.trim(), "json" | "xml" | "catalog" | "watch"),
                "only lowercase tokens parse: {text:?}"
            );
        }
        Err(msg) => {
            assert!(
                !msg.contains('\u{2014}'),
                "format errors must not use an em dash: {msg}"
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
        !hint.contains('\u{2014}'),
        "unknown_list_format must not use an em dash: {hint}"
    );
});
