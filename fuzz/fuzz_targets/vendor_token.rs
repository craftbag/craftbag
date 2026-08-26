#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    match craftbag::SkillSource::parse_vendor_token(&text) {
        Ok(Some(name)) => {
            assert!(
                craftbag::SkillSource::VENDOR_TOKENS.contains(&name.as_str()),
                "only frozen vendor tokens parse: {name:?} from {text:?}"
            );
        }
        Ok(None) => {
            assert!(
                text.trim().is_empty(),
                "only whitespace-only vendor tokens omit: {text:?}"
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
                "vendor errors must stay one line without an em dash: {msg:?}"
            );
            assert!(
                msg.contains("unknown vendor:"),
                "parse errors must name the miss: {msg}"
            );
        }
    }
});
