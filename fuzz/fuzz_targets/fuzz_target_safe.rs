#![no_main]
use std::{borrow::Cow, sync::OnceLock};

use libfuzzer_sys::fuzz_target;
use regex::Regex;
use urldecoder::decode_str;
use urlencoding::decode;
static REGEX: OnceLock<Regex> = OnceLock::new();

#[cfg(not(feature = "safe"))]
compile_error!("This target must be built with the 'safe' feature enabled!");

pub fn decode_url_in_code_safe(code: &str, escape_space: bool) -> (String, bool) {
    if !code.contains("http") {
        return (code.to_string(), false);
    }
    let mut replaced = false;
    let regex = REGEX.get_or_init(|| {
        Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap()
    });

    (
        regex
            .replace_all(code, |caps: &regex::Captures| {
                let url = &caps[0];
                if url.rfind('%').is_none() {
                    return url.to_owned();
                }
                let mut decoded_url = decode(url).unwrap_or(Cow::Borrowed(url));
                let result = if escape_space {
                    decoded_url.to_mut().replace(' ', "%20")
                } else {
                    decoded_url.into()
                };
                if url == result {
                    return url.to_owned();
                }
                replaced = true;
                result
            })
            .into_owned(),
        replaced,
    )
}

// =================================================================
// 2. Fuzz Target
// =================================================================
fuzz_target!(|data: &[u8]| {
    // 步骤 1: 将随机字节转换为 UTF-8 字符串
    // 如果你的解码器也支持非 UTF-8 输入，你可以去掉这个检查直接传 bytes
    if let Ok(input_str) = std::str::from_utf8(data) {
        // 测试场景 A: escape_space = true
        {
            let res = decode_str(input_str, true, false);
            if res.is_err() {
                panic!("Input: {:?}\nMy impl crashed: {:?}", input_str, res);
            }
            let (my_out, my_changed) = res.unwrap();
            let (ref_out, ref_changed) = decode_url_in_code_safe(input_str, true);

            assert_eq!(
                my_out, ref_out,
                "\n[Mismatch escape_space=TRUE]\nInput: {:?}\nMy Output: {:?}\nRef Output: {:?}",
                input_str, my_out, ref_out
            );
            assert_eq!(
                my_changed, ref_changed,
                "\n[Changed Flag Mismatch escape_space=TRUE]\nInput: {:?}",
                input_str
            );
        }

        // escape_space = false
        {
            let res = decode_str(input_str, false, false);
            if res.is_err() {
                panic!("Input: {:?}\nMy impl crashed: {:?}", input_str, res);
            }
            let (my_out, my_changed) = res.unwrap();
            let (ref_out, ref_changed) = decode_url_in_code_safe(input_str, false);

            assert_eq!(
                my_out, ref_out,
                "\n[Mismatch escape_space=FALSE]\nInput: {:?}\nMy Output: {:?}\nRef Output: {:?}",
                input_str, my_out, ref_out
            );
            assert_eq!(
                my_changed, ref_changed,
                "\n[Changed Flag Mismatch escape_space=FALSE]\nInput: {:?}",
                input_str
            );
        }
    }
});
