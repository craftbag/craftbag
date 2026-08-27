#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let out = craftbag::sanitize_error_token(&text);
    assert!(
        !out.contains('\u{2014}')
            && !out.contains('\u{2028}')
            && !out.contains('\u{2029}')
            && !out.contains('\n')
            && !out.contains('\r')
            && !out.contains('\0'),
        "sanitized tokens must stay one line without an em dash: {out:?}"
    );
});
