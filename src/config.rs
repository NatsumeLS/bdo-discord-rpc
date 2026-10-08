use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::phase::Phase;

const DEFAULT_CLIENT_ID: &str = "1551141273551904848";
const CONFIG_FILE: &str = "config.toml";

const PROFILE_BUTTON_LABEL: &str = "Adventurer Profile";
const PROFILE_BUTTON_URL: &str = "{profile_url}";

const GAME_ICON: &str = "https://cdn.patchbot.io/games/25/black-desert-online_1780346031_sm.webp";

/// The steps that bring a config up to one version from the one before.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Step {
    /// What the config was called before this version.
    config_from: Option<String>,
    #[serde(rename = "move")]
    moves: BTreeMap<String, String>,
    delete: Vec<String>,
    reset: Option<Reset>,
    rename: BTreeMap<String, String>,
    remove: Vec<String>,
}

/// Everything back to its default but the tables in `keep`, with the old
/// file written to `backup` first.
#[derive(Deserialize)]
struct Reset {
    keep: Vec<String>,
    backup: String,
}

fn steps() -> &'static BTreeMap<u32, Step> {
    static STEPS: OnceLock<BTreeMap<u32, Step>> = OnceLock::new();
    STEPS.get_or_init(|| {
        toml::from_str::<BTreeMap<String, Step>>(include_str!("../assets/migrations.toml"))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(version, step)| Some((version.parse().ok()?, step)))
            .collect()
    })
}

/// The config layout this build writes, the newest step it knows.
fn version() -> u32 {
    steps().keys().max().copied().unwrap_or_default()
}

pub enum Migration {
    Current,
    /// From this version, and where the old file went when a step reset it.
    Migrated(u32, Option<PathBuf>),
    /// Written by a newer version, whose keys this one ignores.
    Newer(u32),
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    // Its own default, so a config written before versions reads as 0.
    #[serde(default)]
    pub version: u32,
    pub enabled: bool,
    pub language: String,
    pub client_id: String,
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
            "{territory} - {node}",
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
            version: version(),
            enabled: true,
            language: "auto".into(),
            client_id: DEFAULT_CLIENT_ID.into(),
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
        self.servers
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
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

    fn key(self) -> &'static str {
        match self {
            Table::Servers => "servers",
            Table::Characters => "characters",
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
    config_path().with_file_name("bdo-discord-rpc.log")
}

// Only when `path` is missing, so no process writes the defaults over the name
// the file still has. Before any step, since steps run on the file read.
fn adopt_old_name(path: &Path) -> bool {
    // A rename replaces its target, and a file that failed to read may exist.
    !path.exists()
        && steps()
            .values()
            .rev()
            .filter_map(|step| beside_exe(step.config_from.as_deref()?))
            .any(|old| std::fs::rename(old, path).is_ok())
}

pub fn data_dir() -> PathBuf {
    config_path().with_file_name("data")
}

pub fn profile_cache_path() -> PathBuf {
    data_dir().join("profile.json")
}

pub fn status_path() -> PathBuf {
    data_dir().join("status.json")
}

pub fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn load_or_create(path: &Path) -> Result<Config, String> {
    if !path.exists() && !adopt_old_name(path) {
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

    Ok(config)
}

/// Brings an older file up to `version()` in place, leaving the rest as
/// written. Only the tray calls it, so two processes never migrate at once. A
/// file that does not parse is left to the watcher, which reports it.
pub fn migrate(path: &Path) -> Result<Migration, String> {
    let read = || std::fs::read_to_string(path).ok();
    let Some(mut doc) = read()
        .or_else(|| adopt_old_name(path).then(read).flatten())
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
    else {
        return Ok(Migration::Current);
    };
    let from = doc
        .get("version")
        .and_then(toml_edit::Item::as_integer)
        .map_or(0, |v| u32::try_from(v).unwrap_or(0));
    let to = version();
    if from > to {
        return Ok(Migration::Newer(from));
    }
    if from == to {
        return Ok(Migration::Current);
    }
    let mut backup = None;
    for step in steps().range(from + 1..).map(|(_, step)| step) {
        backup = apply(step, &mut doc)?.or(backup);
    }
    doc["version"] = toml_edit::value(i64::from(to));
    write_atomic(path, &doc.to_string())?;
    Ok(Migration::Migrated(from, backup))
}

/// A path under the exe's folder, or none for one that would leave it.
fn beside_exe(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let inside = !name.is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
    inside.then(|| config_path().with_file_name(path))
}

/// Where a reset put the old file, if this step has one. A backup that cannot
/// be written stops the whole migration, so no reset runs without it.
fn apply(step: &Step, doc: &mut toml_edit::DocumentMut) -> Result<Option<PathBuf>, String> {
    for (from, to) in &step.moves {
        let (Some(from), Some(to)) = (beside_exe(from), beside_exe(to)) else {
            continue;
        };
        if !from.exists() {
            continue;
        }
        // Never over a newer copy, which the old one is then no use beside.
        if to.exists() {
            let _ = std::fs::remove_file(&from);
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&from, &to);
    }
    for path in step.delete.iter().filter_map(|name| beside_exe(name)) {
        let _ = std::fs::remove_file(path);
    }
    let mut backup = None;
    if let Some(reset) = &step.reset {
        let path = beside_exe(&reset.backup)
            .ok_or_else(|| format!("{} is outside the exe's Folder", reset.backup))?;
        write_atomic(&path, &doc.to_string())?;
        let defaults = toml::to_string_pretty(&Config::default())
            .map_err(|e| format!("Serializing the Config: {e}"))?;
        let mut fresh: toml_edit::DocumentMut = defaults
            .parse()
            .map_err(|e| format!("Parsing the Defaults: {e}"))?;
        for key in &reset.keep {
            if let Some(kept) = doc.remove(key) {
                fresh.insert(key, kept);
            }
        }
        *doc = fresh;
        backup = Some(path);
    }
    for (from, to) in &step.rename {
        if let Some(value) = take(doc, from) {
            if let Some((table, key)) = parent(doc, to, true) {
                if !table.contains_key(key) {
                    table.insert(key, value);
                }
            }
        }
    }
    for key in &step.remove {
        take(doc, key);
    }
    Ok(backup)
}

fn take(doc: &mut toml_edit::DocumentMut, key: &str) -> Option<toml_edit::Item> {
    let (table, key) = parent(doc, key, false)?;
    table.remove(key)
}

/// The table a dotted key sits in, and its last part, with the tables on the
/// way made when `create` is set.
fn parent<'a, 'k>(
    doc: &'a mut toml_edit::DocumentMut,
    key: &'k str,
    create: bool,
) -> Option<(&'a mut dyn toml_edit::TableLike, &'k str)> {
    let (path, last) = match key.rsplit_once('.') {
        Some((path, last)) => (Some(path), last),
        None => (None, key),
    };
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for part in path.into_iter().flat_map(|path| path.split('.')) {
        let item = if create {
            table.entry(part).or_insert(toml_edit::table())
        } else {
            table.get_mut(part)?
        };
        table = item.as_table_like_mut()?;
    }
    Some((table, last))
}

/// Adds one entry to the file as it is on disk, leaving the rest as written.
pub fn add_name(path: &Path, table: Table, key: &str, name: &str) -> Result<(), String> {
    let parsing = |e: &dyn std::fmt::Display| format!("Parsing {}: {e}", path.display());
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("Reading {}: {e}", path.display()))?;
    toml::from_str::<Config>(&text).map_err(|e| parsing(&e))?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| parsing(&e))?;
    doc.entry(table.key())
        .or_insert(toml_edit::table())
        .as_table_like_mut()
        .ok_or_else(|| format!("[{}] is not a Table", table.key()))?
        .insert(key, toml_edit::value(name));
    write_atomic(path, &doc.to_string())
}

pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    let body =
        toml::to_string_pretty(config).map_err(|e| format!("Serializing the Config: {e}"))?;
    write_atomic(path, &body)
}

/// Written beside the file and renamed over it, so no reader sees it half written.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    path.parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temp, text))
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("Writing {}: {e}", path.display())
        })
}
