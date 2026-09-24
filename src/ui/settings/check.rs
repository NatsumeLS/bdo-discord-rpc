use std::path::Path;

use rust_i18n::t;

use crate::show::presence::{is_placeholder, BUTTON_LABEL_MAX, BUTTON_URL_MAX};
use crate::ui::m3;

pub fn accent(value: &str) -> Option<String> {
    m3::parse_hex(value)
        .is_none()
        .then(|| t!("check.accent").into_owned())
}

pub fn app_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(t!("check.app_id_needed").into_owned());
    }
    (!value.chars().all(|c| c.is_ascii_digit())).then(|| t!("check.app_id_digits").into_owned())
}

fn folder(value: &str, marker: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let path = Path::new(value);
    if !path.is_dir() {
        return Some(t!("check.no_folder").into_owned());
    }
    (!path.join(marker).exists()).then(|| t!("check.no_marker", marker = marker).into_owned())
}

pub fn game_folder(value: &str) -> Option<String> {
    folder(value, "bin64")
}

pub fn user_data(value: &str) -> Option<String> {
    folder(value, "GameOption.txt")
}

pub fn url(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.starts_with("http://") && !value.starts_with("https://"))
        .then(|| t!("check.https").into_owned())
}

pub fn image(value: &str) -> Option<String> {
    let value = value.trim();
    if !value.contains("://") {
        return None;
    }
    if !value.starts_with("https://") {
        return Some(t!("check.image_https").into_owned());
    }
    let path = value.split(['?', '#']).next().unwrap_or(value);
    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    (!matches!(
        extension.as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif")
    ))
    .then(|| t!("check.image_type").into_owned())
}

pub fn template(value: &str) -> Option<String> {
    let snippet = |from: &str| from.chars().take(24).collect::<String>();
    let mut rest = value;

    loop {
        let open = rest.find('{');
        let head = &rest[..open.unwrap_or(rest.len())];
        if let Some(at) = head.find('}') {
            return Some(t!("check.closes_nothing", text = snippet(&head[at..])).into_owned());
        }

        let after = &rest[open?..];
        let Some(close) = after.find('}') else {
            return Some(t!("check.missing_close", text = snippet(after)).into_owned());
        };

        let name = &after[1..close];
        if !is_placeholder(name) {
            return Some(t!("check.no_placeholder", name = name).into_owned());
        }
        rest = &after[close + 1..];
    }
}

pub fn button_label(value: &str) -> Option<String> {
    (!value.contains('{') && value.trim().chars().count() > BUTTON_LABEL_MAX)
        .then(|| t!("check.at_most", max = BUTTON_LABEL_MAX).into_owned())
}

pub fn button_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains('{') {
        return None;
    }
    if value.len() > BUTTON_URL_MAX {
        return Some(t!("check.at_most", max = BUTTON_URL_MAX).into_owned());
    }
    url(value)
}

pub fn name(value: &str) -> Option<String> {
    value
        .trim()
        .is_empty()
        .then(|| t!("prompt.blank").into_owned())
}

pub fn character_id(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return Some(t!("check.id_blank").into_owned());
    }
    (!key.chars().all(|c| c.is_ascii_digit())).then(|| t!("check.id_digits").into_owned())
}
