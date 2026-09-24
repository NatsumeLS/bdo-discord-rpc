use std::collections::HashMap;
use std::sync::Mutex;

use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

use crate::config::Config;

pub const AUTO: &str = "auto";
const FALLBACK: &str = "en";

pub fn available() -> Vec<String> {
    let mut locales: Vec<String> = rust_i18n::available_locales!()
        .into_iter()
        .map(|locale| locale.into_owned())
        .collect();
    locales.sort_unstable();
    locales
}

pub fn apply(config: &Config) {
    let wanted = match config.language.trim() {
        "" | AUTO => windows_languages(),
        chosen => vec![chosen.to_string()],
    };
    let locale = wanted
        .iter()
        .find_map(|tag| supported(tag))
        .unwrap_or_else(|| FALLBACK.to_string());
    if *rust_i18n::locale() != *locale {
        rust_i18n::set_locale(&locale);
    }
}

// `en-US` falls back to `en`, the only form a locale file is named by here.
fn supported(tag: &str) -> Option<String> {
    let locales = available();
    let base = tag.split(['-', '_']).next().unwrap_or(tag);
    [tag, base].into_iter().find_map(|candidate| {
        locales
            .iter()
            .find(|locale| locale.eq_ignore_ascii_case(candidate))
            .cloned()
    })
}

fn windows_languages() -> Vec<String> {
    let mut count = 0u32;
    let mut size = 0u32;
    unsafe {
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut size,
        ) == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; size as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            buffer.as_mut_ptr(),
            &mut size,
        ) == 0
        {
            return Vec::new();
        }
        buffer
            .split(|&unit| unit == 0)
            .filter(|tag| !tag.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

pub fn tr(key: &str) -> &'static str {
    cached(&rust_i18n::locale(), key)
}

pub fn name(locale: &str) -> &'static str {
    cached(locale, "language.name")
}

// Leaked once per key and language, so widgets can hold a `&'static str`.
fn cached(locale: &str, key: &str) -> &'static str {
    static CACHE: Mutex<Option<HashMap<(String, String), &'static str>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(HashMap::new)
        .entry((locale.to_string(), key.to_string()))
        .or_insert_with(|| {
            Box::leak(
                rust_i18n::t!(key, locale = locale)
                    .into_owned()
                    .into_boxed_str(),
            )
        })
}
