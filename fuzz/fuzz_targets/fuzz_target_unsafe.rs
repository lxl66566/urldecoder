#![no_main]
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use regex::Regex;
use urldecoder::decode_str;
use urlencoding::decode;

static REGEX: OnceLock<Regex> = OnceLock::new();

/// 这是一个 "Strict" 版本的参考实现
/// 它不会吞掉 decode 的错误，而是将错误向上传递
pub fn decode_url_strict(code: &str, escape_space: bool) -> Result<(String, bool), ()> {
    if !code.contains("http") {
        return Ok((code.to_string(), false));
    }

    let regex = REGEX.get_or_init(|| {
        Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap()
    });

    let mut replaced = false;
    let mut decode_error = false;

    // 使用 replace_all，但在闭包中捕获 decode_error 状态
    let result_cow = regex.replace_all(code, |caps: &regex::Captures| {
        // 如果之前已经出错了，直接返回原串，不做多余处理
        if decode_error {
            return caps[0].to_string();
        }

        let url = &caps[0];
        if url.rfind('%').is_none() {
            return url.to_owned();
        }

        // 核心改动：不再 unwrap_or，而是处理 Result
        match decode(url) {
            Ok(decoded_cow) => {
                let result = if escape_space {
                    // 如果需要转义空格，这里处理。
                    // 注意：urlencoding::decode 返回的是 Cow，replace 返回 String
                    decoded_cow.replace(' ', "%20")
                } else {
                    decoded_cow.into_owned()
                };

                if url != result {
                    replaced = true;
                }
                result
            }
            Err(_) => {
                // 如果标准库解码失败（例如无效的 UTF-8 序列），标记错误
                decode_error = true;
                url.to_owned() // 返回原串以满足闭包类型，但外部会检查 decode_error
            }
        }
    });

    if decode_error {
        Err(())
    } else {
        Ok((result_cow.into_owned(), replaced))
    }
}

fn test_scenario(input_str: &str, escape_space: bool) {
    let my_res = decode_str(input_str, escape_space, false);
    let ref_res = decode_url_strict(input_str, escape_space);

    match (ref_res, my_res) {
        // Case 1: 参考实现认为这是错误的编码，但你的实现成功解码了。
        // 根据要求：如果 decode(url) 返回 err，decode_str 也需要返回 err。
        (Err(_), Ok(res)) => {
            panic!(
                "\n[Safety Violation]\nInput: {:?}\nRef Impl: Error (Invalid encoding)\nMy Impl: Ok({:?})\nExpectation: My Impl should also return Err.",
                input_str, res
            );
        }

        // Case 2: 参考实现成功，但你的实现报错了。
        (Ok(ref_val), Err(e)) => {
            panic!(
                "\n[False Negative]\nInput: {:?}\nRef Impl: Ok({:?})\nMy Impl: Error({:?})\n",
                input_str, ref_val, e
            );
        }

        // Case 3: 两者都报错，符合预期。
        (Err(_), Err(_)) => {
            return;
        }

        // Case 4: 两者都成功，对比结果。
        (Ok((ref_out, ref_changed)), Ok((my_out, my_changed))) => {
            assert_eq!(
                my_out, ref_out,
                "\n[Output Mismatch escape_space={}]\nInput: {:?}\nMy Output: {:?}\nRef Output: {:?}",
                escape_space, input_str, my_out, ref_out
            );
            assert_eq!(
                my_changed, ref_changed,
                "\n[Changed Flag Mismatch escape_space={}]\nInput: {:?}",
                escape_space, input_str
            );
        }
    }
}

// =================================================================
// 2. Fuzz Target
// =================================================================
fuzz_target!(|data: &[u8]| {
    if let Ok(input_str) = std::str::from_utf8(data) {
        // 测试 escape_space = true
        test_scenario(input_str, true);
        // 测试 escape_space = false
        test_scenario(input_str, false);
    }
});
