use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::phase::Phase;

const DEFAULT_CLIENT_ID: &str = "1551141273551904848";
const CONFIG_FILE: &str = "bdo-discord-rpc.toml";

const PROFILE_BUTTON_LABEL: &str = "Adventurer Profile";
const PROFILE_BUTTON_URL: &str = "{profile_url}";

const GAME_ICON: &str = "https://cdn.patchbot.io/games/25/black-desert-online_1780346031_sm.webp";

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub language: String,
    pub client_id: String,
    pub debounce_seconds: u64,
    pub min_update_seconds: u64,
    pub poll_seconds: u64,
    pub prompt_unknown_server: bool,
    pub prompt_unknown_character: bool,

    pub identity: Identity,
    pub display: Display,
    pub theme: Theme,
    pub paths: Paths,
    pub profile: ProfileConfig,
    pub servers: BTreeMap<String, String>,
    pub characters: BTreeMap<String, String>,
    pub phases: BTreeMap<String, PhaseConfig>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Identity {
    pub family_name: String,
    pub show_family: bool,
    pub show_character: bool,
    pub region_name: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Display {
    pub show_server: bool,
    pub show_region: bool,
    pub timer_mode: String,
    pub game_icon: String,
    pub unknown: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Theme {
    pub mode: String,
    pub accent: String,
    pub palette: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            mode: "dark".into(),
            accent: "F08080".into(),
            palette: "tonal_spot".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Paths {
    pub game_root: String,
    pub user_data_dir: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ProfileConfig {
    pub enabled: bool,
    pub url: String,
    pub refresh_minutes: u64,
    pub search_url: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct PhaseConfig {
    pub details: String,
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    pub small_image: String,
    pub small_text: String,
    pub button_label: String,
    pub button_url: String,
    pub second_button_label: String,
    pub second_button_url: String,
    pub report: bool,
}

impl Default for Identity {
    fn default() -> Self {
        Identity {
            family_name: String::new(),
            show_family: true,
            show_character: true,
            region_name: String::new(),
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Display {
            show_server: true,
            show_region: true,
            timer_mode: "session".into(),
            game_icon: GAME_ICON.into(),
            unknown: "Unknown".into(),
        }
    }
}

impl Default for ProfileConfig {
    fn default() -> Self {
        ProfileConfig {
            enabled: true,
            url: String::new(),
            refresh_minutes: 60,
            search_url: String::new(),
        }
    }
}

impl Default for PhaseConfig {
    fn default() -> Self {
        PhaseConfig {
            details: String::new(),
            state: String::new(),
            large_image: String::new(),
            large_text: String::new(),
            small_image: String::new(),
            small_text: String::new(),
            button_label: PROFILE_BUTTON_LABEL.into(),
            button_url: PROFILE_BUTTON_URL.into(),
            second_button_label: String::new(),
            second_button_url: String::new(),
            report: true,
        }
    }
}

fn phase_defaults(phase: Phase) -> PhaseConfig {
    let (details, state, large_image, large_text, small_text) = match phase {
        Phase::Home => ("Starting up", "{family}", "", "{region}", ""),
        Phase::Login => ("In the Main Menu", "{family}", "", "{region}", ""),
        Phase::ServerSelect => ("Choosing a Server", "{family}", "", "{region}", ""),
        Phase::CharacterCreate => ("Creating a Character", "{family}", "", "{region}", ""),
        Phase::Lobby => ("Selecting a Character", "{family}", "", "{region}", ""),
        Phase::Loading => ("Loading", "{family}", "", "{region}", ""),
        Phase::Play => (
            "{character}",
            "{family}",
            "{class_image}",
            "{class} - Lv. {level}",
            "{region} - {server}",
        ),
    };

    PhaseConfig {
        details: details.into(),
        state: state.into(),
        large_image: large_image.into(),
        large_text: large_text.into(),
        small_text: small_text.into(),
        ..PhaseConfig::default()
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            language: "auto".into(),
            client_id: DEFAULT_CLIENT_ID.into(),
            debounce_seconds: 4,
            min_update_seconds: 15,
            poll_seconds: 3,
            prompt_unknown_server: true,
            prompt_unknown_character: true,
            identity: Identity::default(),
            display: Display::default(),
            theme: Theme::default(),
            paths: Paths::default(),
            profile: ProfileConfig::default(),
            servers: BTreeMap::new(),
            characters: BTreeMap::new(),
            phases: Phase::ALL
                .iter()
                .map(|&p| (p.key().to_string(), phase_defaults(p)))
                .collect(),
        }
    }
}

impl Config {
    pub fn phase(&self, phase: Phase) -> PhaseConfig {
        self.phases
            .get(phase.key())
            .cloned()
            .unwrap_or_else(|| phase_defaults(phase))
    }

    pub fn server_name(&self, key: &str) -> String {
        match self.servers.get(key) {
            Some(name) => name.clone(),
            None => key.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Servers,
    Characters,
}

impl Table {
    pub fn entries(self, config: &Config) -> &BTreeMap<String, String> {
        match self {
            Table::Servers => &config.servers,
            Table::Characters => &config.characters,
        }
    }

    pub fn entries_mut(self, config: &mut Config) -> &mut BTreeMap<String, String> {
        match self {
            Table::Servers => &mut config.servers,
            Table::Characters => &mut config.characters,
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Table::Servers => "Server",
            Table::Characters => "Character",
        }
    }
}

pub fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(CONFIG_FILE)))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
}

pub fn log_path() -> PathBuf {
    config_path().with_extension("log")
}

pub fn profile_cache_path() -> PathBuf {
    config_path().with_file_name("profile.json")
}

pub fn status_path() -> PathBuf {
    config_path().with_file_name("status.json")
}

pub fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn load_or_create(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        let config = Config::default();
        save(path, &config)?;
        return Ok(config);
    }

    let text =
        std::fs::read_to_string(path).map_err(|e| format!("Reading {}: {e}", path.display()))?;
    let mut config: Config =
        toml::from_str(&text).map_err(|e| format!("Parsing {}: {e}", path.display()))?;

    for phase in Phase::ALL {
        config
            .phases
            .entry(phase.key().to_string())
            .or_insert_with(|| phase_defaults(phase));
    }

    config.debounce_seconds = config.debounce_seconds.min(120);
    config.min_update_seconds = config.min_update_seconds.clamp(5, 600);
    config.poll_seconds = config.poll_seconds.clamp(1, 60);
    config.profile.refresh_minutes = config.profile.refresh_minutes.clamp(15, 1440);

    Ok(config)
}

pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    let body =
        toml::to_string_pretty(config).map_err(|e| format!("Serializing the Config: {e}"))?;
    std::fs::write(path, body).map_err(|e| format!("Writing {}: {e}", path.display()))
}
