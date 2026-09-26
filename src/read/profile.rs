use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use chrono::Local;
use scraper::selectable::Selectable;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

const USER_AGENT: &str = concat!("bdo-discord-rpc/", env!("CARGO_PKG_VERSION"));
const TIMEOUT: Duration = Duration::from_secs(15);
// What the page puts in place of a value its owner has not made public.
const LOCK: &str = "em.lock";

#[derive(Serialize, Deserialize)]
pub struct Character {
    pub name: String,
    pub class: String,
    pub level: Option<u32>,
    pub is_main: bool,
    pub class_image: Option<String>,
}

// In the order of the page's `icn_spec01` to `icn_spec11` icons.
pub const LIFE_SKILLS: [&str; 11] = [
    "Gathering",
    "Fishing",
    "Hunting",
    "Cooking",
    "Alchemy",
    "Processing",
    "Training",
    "Trading",
    "Farming",
    "Sailing",
    "Barter",
];

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub family: Option<String>,
    pub guild: Option<String>,
    pub gear_score: Option<String>,
    pub energy: Option<String>,
    pub contribution: Option<String>,
    pub created: Option<String>,
    pub life_skills: BTreeMap<String, String>,
    pub characters: Vec<Character>,
    pub url: Option<String>,
    pub fetched_at: i64,
    pub hidden: bool,
}

impl Character {
    pub fn summary(&self) -> String {
        format!(
            "{} ({} {})",
            self.name,
            self.class,
            self.level.map(|l| l.to_string()).unwrap_or_default()
        )
    }
}

impl Profile {
    pub fn main(&self) -> Option<&Character> {
        self.characters
            .iter()
            .find(|c| c.is_main)
            .or_else(|| self.characters.first())
    }

    pub fn character(&self, name: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.name == name)
    }

    // "Nov 10, 2023, 18:53 (UTC+8)" reads as "Nov 10, 2023", and each
    // region's own format loses its time the same way.
    pub fn created_date(&self) -> Option<String> {
        let created = self.created.as_deref()?.split(" (").next()?.trim();
        let date = match created.rsplit_once(' ') {
            Some((date, time)) if time.contains(':') => date,
            _ => created,
        };
        Some(date.trim_end_matches(',').to_string())
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(TIMEOUT)
        .timeout_read(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
}

fn get(url: &str, what: &str) -> Result<String, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| format!("{what}: {e}"))?
        .into_string()
        .map_err(|e| format!("{what}, reading the Response: {e}"))
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub fn resolve_url(search: &str, family: &str) -> Result<String, String> {
    let search = search.trim();
    if search.is_empty() {
        return Err("No Search URL for this Region".into());
    }

    let url = if search.contains("{family}") {
        search.replace("{family}", &encode(family))
    } else {
        // A bare URL from an older config. `_type=2` is Family Name, and
        // `_type=1` searches Character Names and finds nothing for a family.
        format!("{search}?_type=2&_keyword={}", encode(family))
    };
    let body = get(&url, &format!("Searching for {family}"))?;

    pick_profile_link(&body, family)
        .ok_or_else(|| format!("No Profile found for {family}, is it public?"))
}

fn pick_profile_link(html: &str, family: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let anchors = Selector::parse(r#"a[href*='Profile/Adventure']"#).ok()?;

    let mut first = None;
    for a in doc.select(&anchors) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        if collapse(&a.text().collect::<String>()).eq_ignore_ascii_case(family) {
            return Some(href.to_string());
        }
        first.get_or_insert_with(|| href.to_string());
    }

    first
}

pub fn fetch(url: &str) -> Result<Profile, String> {
    if url.trim().is_empty() {
        return Err("No Profile URL set in the Config".into());
    }

    let mut profile = parse(&get(url, "Fetching the Profile")?);
    profile.url = Some(url.to_string());
    profile.fetched_at = Local::now().timestamp();

    if profile.characters.is_empty() && profile.family.is_none() {
        return Err("The Profile Page had no readable Data, is it public?".into());
    }
    Ok(profile)
}

pub fn parse(html: &str) -> Profile {
    let doc = Html::parse_document(html);
    let mut profile = Profile::default();

    if let (Ok(list), Ok(main_label)) = (
        Selector::parse("ul.character_list > li"),
        Selector::parse("span.selected_label"),
    ) {
        for item in doc.select(&list) {
            let Some(name) = text_of(item, "p.character_name") else {
                continue;
            };

            // The main label sits inside the name element.
            let is_main = item.select(&main_label).next().is_some();
            let label = text_of(item, "span.selected_label").unwrap_or_default();
            let name = name.replace(&label, "").trim().to_string();
            if name.is_empty() {
                continue;
            }

            profile.characters.push(Character {
                name,
                class: text_of(item, "p.character_info em:not(.icon_symbol)").unwrap_or_default(),
                level: text_of(item, "p.character_info span:last-child")
                    .and_then(|t| parse_level(&t)),
                is_main,
                class_image: attr_of(item, "img.icon_character_image", "src").map(strip_query),
            });
        }
    }

    profile.family = text_of(&doc, "p.nick");
    // By position, since every label is in the site's language.
    let stats = stats(&doc);
    let stat = |index: usize| stats.get(index).cloned().flatten();
    profile.created = stat(0);
    profile.guild = stat(1);
    profile.gear_score = stat(2);
    profile.energy = stat(3);
    profile.contribution = stat(4);
    profile.hidden = Selector::parse(LOCK).is_ok_and(|lock| doc.select(&lock).next().is_some());

    if let (Ok(skills), Ok(level), Ok(icon)) = (
        Selector::parse("ul.character_spec > li"),
        Selector::parse("span.spec_level"),
        Selector::parse("span.icon_spec"),
    ) {
        for item in doc.select(&skills) {
            let Some(name) = item
                .select(&icon)
                .next()
                .and_then(|found| found.value().classes().find_map(skill_of))
            else {
                continue;
            };
            // "Skilled<em>9</em>" has no space of its own between the two.
            let grade = item
                .select(&level)
                .next()
                .map(|found| collapse(&found.text().collect::<Vec<_>>().join(" ")))
                .filter(|text| !text.is_empty());
            if let Some(grade) = grade {
                profile.life_skills.insert(name.to_string(), grade);
            }
        }
    }

    profile
}

// Created, guild, gear score, energy, contribution, with None for a hidden one.
fn stats(doc: &Html) -> Vec<Option<String>> {
    let Ok(items) = Selector::parse("ul.line_list > li") else {
        return Vec::new();
    };
    doc.select(&items)
        .map(|item| {
            text_of(item, LOCK)
                .is_none()
                .then(|| text_of(item, "span.desc"))
                .flatten()
        })
        .collect()
}

// "icn_spec01" is the first of `LIFE_SKILLS`.
fn skill_of(class: &str) -> Option<&'static str> {
    let index: usize = class.strip_prefix("icn_spec")?.parse().ok()?;
    LIFE_SKILLS.get(index.checked_sub(1)?).copied()
}

fn text_of<'a>(within: impl Selectable<'a>, selector: &str) -> Option<String> {
    let sel = Selector::parse(selector).ok()?;
    let found = within.select(&sel).next()?;
    let text = collapse(&found.text().collect::<String>());
    (!text.is_empty()).then_some(text)
}

fn attr_of<'a>(within: impl Selectable<'a>, selector: &str, attr: &str) -> Option<String> {
    let sel = Selector::parse(selector).ok()?;
    let found = within.select(&sel).next()?;
    found.value().attr(attr).map(str::to_string)
}

fn strip_query(url: String) -> String {
    match url.split_once('?') {
        Some((base, _)) => base.to_string(),
        None => url,
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_level(text: &str) -> Option<u32> {
    text.chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

pub fn load_cache(path: &Path) -> Option<Profile> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save_cache(path: &Path, profile: &Profile) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(profile).map_err(|e| format!("Serializing the Cache: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("Writing {}: {e}", path.display()))
}
