#![no_main]
use std::{borrow::Cow, sync::LazyLock};

use libfuzzer_sys::fuzz_target;
use memchr::memmem::Finder;
use regex::Regex;
use urldecoder::{decode_in_place, decode_str};
use urlencoding::decode;

static REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https?://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]"#).unwrap()
});
static HTTP_FINDER: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(b"http"));

#[allow(clippy::result_unit_err)]
pub fn decode_url_strict(code: &str, escape_space: bool) -> Result<(Cow<'_, str>, bool), ()> {
    if HTTP_FINDER.find(b"http").is_none() {
        return Ok((Cow::Borrowed(code), false));
    }

    let mut replaced = false;
    let mut decode_error = false;

    let result_cow = REGEX.replace_all(code, |caps: &regex::Captures| {
        if decode_error {
            return Cow::Borrowed("");
        }

        let url = &caps[0];
        if url.rfind('%').is_none() {
            return Cow::Owned(url.to_owned());
        }

        // 核心改动：不再 unwrap_or，而是处理 Result
        match decode(url) {
            Ok(decoded_cow) => {
                let result = if escape_space {
                    decoded_cow.replace(' ', "%20")
                } else {
                    decoded_cow.into_owned()
                };

                if url != result {
                    replaced = true;
                }
                Cow::Owned(result)
            }
            Err(_) => {
                decode_error = true;
                Cow::Borrowed("")
            }
        }
    });

    if decode_error {
        Err(())
    } else {
        Ok((result_cow, replaced))
    }
}

fn test_basic(input_str: &str, ref_res: Result<&(Cow<str>, bool), &()>, escape_space: bool) {
    let my_res = decode_str(input_str, escape_space);

    match (ref_res, my_res.as_ref()) {
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
        (Err(_), Err(_)) => {}

        // Case 4: 两者都成功，对比结果。
        (Ok((ref_out, ref_changed)), Ok((my_out, my_changed))) => {
            assert_eq!(
                my_out,
                ref_out.as_ref(),
                "\n[Output Mismatch escape_space={}]\nInput: {:?}\nMy Output: {:?}\nRef Output: {:?}",
                escape_space,
                input_str,
                my_out,
                ref_out
            );
            assert_eq!(
                my_changed, ref_changed,
                "\n[Changed Flag Mismatch escape_space={}]\nInput: {:?}",
                escape_space, input_str
            );
        }
    }
}

fn test_in_place(mut input: Vec<u8>, ref_res: (Cow<str>, bool), escape_space: bool) {
    let res = decode_in_place(&mut input, escape_space);
    let (my_res, my_changed) = (&input[0..res], res < input.len());

    assert_eq!(
        my_res,
        ref_res.0.as_bytes(),
        "\n[Output Mismatch escape_space={}]\nInput: {:?}\nMy Output: {:?}\nRef Output: {:?}",
        escape_space,
        input,
        my_res,
        ref_res.0
    );
    assert_eq!(
        my_changed, ref_res.1,
        "\n[Changed Flag Mismatch escape_space={}]\nInput: {:?}",
        escape_space, input
    );
}

// =================================================================
// 2. Fuzz Target
// =================================================================
fuzz_target!(|data: &[u8]| {
    if let Ok(input_str) = std::str::from_utf8(data) {
        let expected = decode_url_strict(input_str, false);
        let expected_escape_space = decode_url_strict(input_str, true);
        test_basic(input_str, expected.as_ref(), false);
        test_basic(input_str, expected_escape_space.as_ref(), true);
        if let Ok(expected) = expected {
            test_in_place(data.to_vec(), expected, false);
            test_in_place(data.to_vec(), expected_escape_space.unwrap(), true);
        }
    }
});
