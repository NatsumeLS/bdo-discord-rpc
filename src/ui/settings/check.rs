use std::path::Path;

use crate::show::presence::{is_placeholder, BUTTON_LABEL_MAX, BUTTON_URL_MAX};
use crate::ui::m3;

pub fn accent(value: &str) -> Option<String> {
    m3::parse_hex(value)
        .is_none()
        .then(|| "Not a Color, using the Default".to_string())
}

pub fn app_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some("Needed to connect to Discord".to_string());
    }
    (!value.chars().all(|c| c.is_ascii_digit())).then(|| "An App ID is all digits".to_string())
}

fn folder(value: &str, marker: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let path = Path::new(value);
    if !path.is_dir() {
        return Some("No such Folder".to_string());
    }
    (!path.join(marker).exists()).then(|| format!("No {marker} here"))
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
        .then(|| "Must start with https://".to_string())
}

pub fn image(value: &str) -> Option<String> {
    let value = value.trim();
    if !value.contains("://") {
        return None;
    }
    if !value.starts_with("https://") {
        return Some("A URL must be https://".to_string());
    }
    let path = value.split(['?', '#']).next().unwrap_or(value);
    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    (!matches!(
        extension.as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif")
    ))
    .then(|| "Must be a PNG, JPEG, WebP or GIF".to_string())
}

pub fn template(value: &str) -> Option<String> {
    let snippet = |from: &str| from.chars().take(24).collect::<String>();
    let mut rest = value;

    loop {
        let open = rest.find('{');
        let head = &rest[..open.unwrap_or(rest.len())];
        if let Some(at) = head.find('}') {
            return Some(format!("{} closes nothing", snippet(&head[at..])));
        }

        let after = &rest[open?..];
        let Some(close) = after.find('}') else {
            return Some(format!("{} is missing its }}", snippet(after)));
        };

        let name = &after[1..close];
        if !is_placeholder(name) {
            return Some(format!("There is no {{{name}}}"));
        }
        rest = &after[close + 1..];
    }
}

pub fn button_label(value: &str) -> Option<String> {
    (!value.contains('{') && value.trim().chars().count() > BUTTON_LABEL_MAX)
        .then(|| format!("At most {BUTTON_LABEL_MAX} Characters"))
}

pub fn button_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains('{') {
        return None;
    }
    if value.len() > BUTTON_URL_MAX {
        return Some(format!("At most {BUTTON_URL_MAX} Characters"));
    }
    url(value)
}

pub fn name(value: &str) -> Option<String> {
    value
        .trim()
        .is_empty()
        .then(|| "A Name cannot be blank".to_string())
}

pub fn character_id(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return Some("An ID cannot be blank".to_string());
    }
    (!key.chars().all(|c| c.is_ascii_digit())).then(|| "A Character ID is all digits".to_string())
}
