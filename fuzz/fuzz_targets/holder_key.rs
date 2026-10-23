//! Key file text through `HolderKey::from_secret_str`: either raw text, or a
//! real key generated inside the target and then edited by the input.
//!
//! Besides never panicking, an error must never repeat its input: key file
//! text is secret, and an error message can end up in a log. Any 16
//! characters of the input showing up in the error counts as a leak.
//! Nothing is ever printed.

#![no_main]

use libfuzzer_sys::arbitrary::{Result as ArbResult, Unstructured};
use libfuzzer_sys::fuzz_target;
use twofold_savings::HolderKey;

thread_local! {
    static GENERATED: &'static str = Box::leak(HolderKey::generate().to_secret_string().into_boxed_str());
}

fn edit(key: &str, u: &mut Unstructured) -> ArbResult<String> {
    let mut chars: Vec<char> = key.chars().collect();
    for _ in 0..u.int_in_range(0..=6)? {
        match u.int_in_range(0..=5)? {
            0 if !chars.is_empty() => {
                let i = u.choose_index(chars.len())?;
                chars[i] = u.arbitrary()?;
            }
            1 => {
                let i = u.int_in_range(0..=chars.len())?;
                chars.insert(i, u.arbitrary()?);
            }
            2 if !chars.is_empty() => {
                let i = u.choose_index(chars.len())?;
                chars.remove(i);
            }
            3 => chars.insert(0, '\u{feff}'),
            4 => chars.push(*u.choose(&['\n', '\r', ' ', '\t', '\0'])?),
            5 => chars.truncate(u.int_in_range(0..=chars.len())?),
            _ => {}
        }
    }
    Ok(chars.into_iter().collect())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let text = if u.arbitrary::<bool>().unwrap_or(false) {
        match GENERATED.with(|key| edit(key, &mut u)) {
            Ok(text) => text,
            Err(_) => return,
        }
    } else {
        String::from_utf8_lossy(u.take_rest()).into_owned()
    };

    match HolderKey::from_secret_str(&text) {
        Ok(key) => {
            let _ = key.public_key(0);
        }
        Err(e) => {
            let message = e.to_string();
            let chars: Vec<char> = text.trim().chars().collect();
            if chars.len() <= 512 {
                for window in chars.windows(16) {
                    let piece: String = window.iter().collect();
                    // Don't print the piece: it may be key text.
                    assert!(!message.contains(&piece), "from_secret_str's error repeats part of its input");
                }
            }
        }
    }
});
