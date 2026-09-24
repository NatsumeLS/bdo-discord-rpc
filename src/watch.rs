use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::config::{self, Config, Table};
use crate::phase::{self, Phase};
use crate::read::game::{self, GameFinder, GameProcess};
use crate::read::log_tail::{GameState, LogTail};
use crate::read::profile::{self, Profile, LIFE_SKILLS};
use crate::region;
use crate::show::discord::Presence;
use crate::show::presence::{build, context, PresenceFields, PLACEHOLDERS};
use crate::ui::lang::tr;
use crate::ui::tray::Health;
use crate::win::{self, log};
use rust_i18n::t;

const CONNECT_MIN_BACKOFF: u64 = 5;
const CONNECT_MAX_BACKOFF: u64 = 60;

const FAMILY_RECHECK: Duration = Duration::from_secs(30);

const PROFILE_MIN_BACKOFF: u64 = 60;
const PROFILE_MAX_BACKOFF: u64 = 900;

struct Backoff {
    min: u64,
    max: u64,
    secs: u64,
    next: Instant,
}

impl Backoff {
    fn new(min: u64, max: u64) -> Self {
        Backoff {
            min,
            max,
            secs: min,
            next: Instant::now(),
        }
    }

    fn due(&self) -> bool {
        Instant::now() >= self.next
    }

    fn hold(&mut self) {
        self.next = Instant::now() + Duration::from_secs(self.secs);
    }

    fn failed(&mut self) -> u64 {
        let wait = self.secs;
        self.hold();
        self.secs = (self.secs * 2).min(self.max);
        wait
    }

    fn succeeded(&mut self) {
        self.secs = self.min;
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Status {
    pub health: Health,
    pub line: String,
}

impl Status {
    fn new(health: Health, line: impl Into<String>) -> Self {
        Status {
            health,
            line: line.into(),
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::new(Health::Idle, tr("status.starting_up"))
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub health: Health,
    pub line: String,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub placeholders: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

impl Snapshot {
    pub fn read(path: &Path) -> Option<Snapshot> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

#[derive(Default)]
pub struct Shared {
    pub status: Mutex<Status>,
    pub quit: AtomicBool,
    pub config_error: AtomicBool,
}

pub fn resolve_game(config: &Config, finder: &mut GameFinder) -> Option<GameProcess> {
    match non_empty(&config.paths.game_root) {
        Some(root) => Some(GameProcess {
            root: root.into(),
            started_at: None,
        }),
        None => finder.find(),
    }
}

pub fn resolve_user_data_dir(config: &Config) -> Option<PathBuf> {
    non_empty(&config.paths.user_data_dir)
        .map(PathBuf::from)
        .or_else(game::default_user_data_dir)
}

pub fn resolve_family(config: &Config) -> Option<String> {
    non_empty(&config.identity.family_name)
        .or_else(|| resolve_user_data_dir(config).and_then(|dir| game::detect_family(&dir)))
}

pub fn resolve_region(config: &Config, service: Option<&str>) -> Option<String> {
    non_empty(&config.identity.region_name).or_else(|| service.map(region::name))
}

// The config's own Search URL, or the one for the region the game runs in.
fn search_url(config: &Config, service: Option<&str>) -> String {
    non_empty(&config.profile.search_url)
        .or_else(|| service.and_then(region::get).map(|r| r.search.clone()))
        .unwrap_or_default()
}

fn past_read_window(play_started: SystemTime) -> bool {
    // The margin absorbs a clock that disagrees slightly with the log.
    play_started
        .elapsed()
        .is_ok_and(|since| since > game::CHARACTER_READ_WINDOW + Duration::from_secs(5))
}

pub fn play_started_at(state: &GameState) -> Option<SystemTime> {
    if state.phase != Some(Phase::Play) {
        return None;
    }
    let secs = u64::try_from(state.phase_since?).ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

pub enum Fetch {
    Skipped,
    Fetched,
    Failed,
}

pub fn refresh_profile(
    config: &Config,
    family: Option<&str>,
    service: Option<&str>,
    url: &mut Option<String>,
    profile: &mut Option<Profile>,
) -> Fetch {
    if !config.profile.enabled {
        *profile = None;
        return Fetch::Skipped;
    }

    let cache_path = config::profile_cache_path();
    if profile.is_none() {
        *profile = profile::load_cache(&cache_path);
        if let Some(cached) = profile.as_ref() {
            log(&format!(
                "Profile: Loaded from the Cache, fetched {}",
                stamp(Some(cached.fetched_at)).unwrap_or_default()
            ));
        }
    }

    let age_limit = (config.profile.refresh_minutes * 60) as i64;
    if profile
        .as_ref()
        .is_some_and(|p| Local::now().timestamp() - p.fetched_at < age_limit)
    {
        return Fetch::Skipped;
    }

    let target = match url {
        Some(url) => url.clone(),
        None => {
            let found = if !config.profile.url.trim().is_empty() {
                config.profile.url.clone()
            } else if let Some(cached) = profile.as_ref().and_then(|p| p.url.clone()) {
                cached
            } else if let Some(family) = family {
                let search = search_url(config, service);
                // A region without a search was already warned about when the game was found.
                if search.is_empty() {
                    return Fetch::Skipped;
                }
                match profile::resolve_url(&search, family) {
                    Ok(found) => {
                        log(&format!("Profile: Found the Page for {family}"));
                        found
                    }
                    Err(e) => {
                        win::warn(&format!("Profile: {e}"));
                        return Fetch::Failed;
                    }
                }
            } else {
                return Fetch::Skipped;
            };
            url.insert(found).clone()
        }
    };

    match profile::fetch(&target) {
        Ok(fetched) => {
            match fetched.main() {
                Some(main) => log(&format!(
                    "Profile: Fetched {} Characters, Main {}",
                    fetched.characters.len(),
                    main.summary()
                )),
                None => win::warn("Profile: Fetched, but it lists no Characters"),
            }
            if let Err(e) = profile::save_cache(&cache_path, &fetched) {
                win::warn(&format!("Profile: Could not save the Cache ({e})"));
            }
            *profile = Some(fetched);
            Fetch::Fetched
        }
        Err(e) => {
            win::warn(&format!("Profile: {e}"));
            Fetch::Failed
        }
    }
}

#[derive(Default)]
struct Derived {
    family: Option<String>,
    last_family_check: Option<Instant>,
    profile: Option<Profile>,
    profile_url: Option<String>,
    unmatched: Option<String>,
    last_sent: Option<PresenceFields>,
}

impl Derived {
    fn new(config: &Config) -> Self {
        Derived {
            family: non_empty(&config.identity.family_name),
            ..Derived::default()
        }
    }
}

// `Game` and `Session` stay out of `Derived`, which a config reload rebuilds,
// or saving a name from the prompt would throw the character reading away.
struct Game {
    root: PathBuf,
    tail: LogTail,
    service: Option<String>,
}

struct Session {
    state: GameState,
    stable: GameState,
    observed: (Option<Phase>, Option<String>),
    observed_since: Instant,
    character: Option<String>,
    read_for: Option<i64>,
    missed_for: Option<i64>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            state: GameState::default(),
            stable: GameState::default(),
            observed: (None, None),
            observed_since: Instant::now(),
            character: None,
            read_for: None,
            missed_for: None,
        }
    }
}

struct Link {
    client: Option<Presence>,
    retry: Backoff,
}

impl Default for Link {
    fn default() -> Self {
        Link {
            client: None,
            retry: Backoff::new(CONNECT_MIN_BACKOFF, CONNECT_MAX_BACKOFF),
        }
    }
}

impl Link {
    fn reset(&mut self) {
        *self = Link::default();
    }

    fn lost(&mut self) {
        self.client = None;
        self.retry.hold();
    }

    fn connect(&mut self, client_id: &str) -> Option<Result<(), String>> {
        if self.client.is_some() || !self.retry.due() {
            return None;
        }
        match Presence::connect(client_id) {
            Ok(presence) => {
                self.client = Some(presence);
                self.retry.succeeded();
                Some(Ok(()))
            }
            Err(e) => {
                self.retry.failed();
                Some(Err(e))
            }
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_string())
}

pub fn watch(shared: &Shared) -> i32 {
    let mut watcher = Watcher::new(shared);
    log("Watching for Black Desert");

    while !shared.quit.load(Ordering::Relaxed) {
        let status = watcher.tick();
        watcher.publish(status);
        let poll = Duration::from_secs(watcher.config.poll_seconds);
        watcher.reload_asked = win::sleep_until_config_changes(poll);
    }

    log("Stopped");
    watcher.prompt.close();
    0
}

struct Watcher<'a> {
    shared: &'a Shared,
    config_path: PathBuf,
    config: Config,
    loaded: bool,
    config_mtime: Option<SystemTime>,
    reload_asked: bool,
    disabled: bool,

    finder: GameFinder,
    game: Option<Game>,
    session: Session,
    derived: Derived,

    link: Link,
    last_push: Option<Instant>,
    profile_retry: Backoff,

    prompted: HashSet<String>,
    prompt: win::ChildWindow,

    status_path: PathBuf,
    published: Option<Snapshot>,
}

impl<'a> Watcher<'a> {
    fn new(shared: &'a Shared) -> Self {
        let config_path = config::config_path();

        let (config, loaded) = match config::load_or_create(&config_path) {
            Ok(config) => {
                log(&format!("Config: Loaded from {}", config_path.display()));
                (config, true)
            }
            Err(e) => {
                win::error(&format!("Config: {e}"));
                win::warn("Config: Standing by until the File is fixed");
                shared.config_error.store(true, Ordering::Relaxed);
                (Config::default(), false)
            }
        };

        // Before the first sleep, or the first Save would be waited out.
        win::listen_for_config_changes();

        Watcher {
            shared,
            config_mtime: config::mtime(&config_path),
            config_path,
            derived: Derived::new(&config),
            config,
            loaded,
            reload_asked: false,
            disabled: false,
            finder: GameFinder::default(),
            game: None,
            session: Session::default(),
            link: Link::default(),
            last_push: None,
            profile_retry: Backoff::new(PROFILE_MIN_BACKOFF, PROFILE_MAX_BACKOFF),
            prompted: HashSet::new(),
            prompt: win::ChildWindow::default(),
            status_path: config::status_path(),
            published: None,
        }
    }

    fn tick(&mut self) -> Status {
        self.reload();

        if !self.loaded {
            return Status::new(Health::Waiting, tr("status.config_error"));
        }

        if !self.config.enabled {
            if !self.disabled {
                self.disabled = true;
                log("Config: Disabled, standing by");
                // Not reset(): the retry schedule survives being switched off.
                self.link.client = None;
                self.last_push = None;
                self.derived.last_sent = None;
                self.game = None;
                self.session = Session::default();
            }
            return Status::new(Health::Idle, tr("status.disabled"));
        }
        if self.disabled {
            self.disabled = false;
            log("Config: Enabled, watching again");
        }

        self.track_game();
        let Some(game) = self.game.as_mut() else {
            return Status::new(Health::Idle, tr("status.waiting_for_game"));
        };
        game.tail.poll(&mut self.session.state);

        self.read_family();
        self.read_character();
        self.read_profile();
        self.check_name();
        self.connect();
        self.debounce();
        self.ask_names();
        self.push();

        match self.link.client {
            Some(_) => Status::new(Health::Live, self.status_line()),
            None => Status::new(Health::Waiting, tr("status.discord_not_connected")),
        }
    }

    fn reload(&mut self) {
        let mtime = config::mtime(&self.config_path);
        let changed = mtime.is_some() && mtime != self.config_mtime;
        if !std::mem::take(&mut self.reload_asked) && !changed {
            return;
        }
        self.config_mtime = mtime;

        match config::load_or_create(&self.config_path) {
            Ok(reloaded) => {
                log(if self.loaded {
                    "Config: Reloaded"
                } else {
                    "Config: Loaded"
                });
                if reloaded.client_id != self.config.client_id {
                    self.link.reset();
                }
                self.config = reloaded;
                crate::ui::lang::apply(&self.config);
                self.derived = Derived::new(&self.config);
                self.loaded = true;
            }
            Err(e) => win::warn(&format!(
                "Config: Reload failed, keeping the previous one ({e})"
            )),
        }
    }

    fn track_game(&mut self) {
        let running = resolve_game(&self.config, &mut self.finder);
        if running.as_ref().map(|p| &p.root) == self.game.as_ref().map(|g| &g.root) {
            return;
        }

        self.game = match running {
            Some(process) => {
                log(&format!("Game: Found at {}", process.root.display()));
                let service = game::detect_service(&process.root);
                match &service {
                    Some(code) if region::get(code).is_none() => win::warn(&format!(
                        "Region: {code} is not in regions.toml, so it has no Server Names or Profile Search"
                    )),
                    _ => {}
                }
                match resolve_region(&self.config, service.as_deref()) {
                    Some(shown) => log(&format!("Region: {shown}")),
                    None => win::warn("Region: Not found in service.ini"),
                }
                Some(Game {
                    tail: LogTail::new(&process.root, process.started_at),
                    service,
                    root: process.root,
                })
            }
            None => {
                log("Game: Closed, clearing the Presence");
                self.link.reset();
                self.derived.last_sent = None;
                self.last_push = None;
                None
            }
        };
        self.session = Session::default();
    }

    fn read_family(&mut self) {
        let due = self
            .derived
            .last_family_check
            .is_none_or(|t| t.elapsed() >= FAMILY_RECHECK);
        if self.derived.family.is_some() || !self.config.identity.show_family || !due {
            return;
        }
        let first_check = self.derived.last_family_check.is_none();
        self.derived.last_family_check = Some(Instant::now());
        self.derived.family = resolve_family(&self.config);
        match &self.derived.family {
            Some(name) => log(&format!("Family: {name}")),
            None if first_check => win::warn("Family: Not found in UserCache, retrying"),
            None => {}
        }
    }

    fn read_character(&mut self) {
        let s = &mut self.session;
        if !self.config.identity.show_character || s.state.phase_since == s.read_for {
            return;
        }
        let Some(started) = play_started_at(&s.state) else {
            return;
        };

        let found = resolve_user_data_dir(&self.config)
            .and_then(|dir| game::detect_character(&dir, started));
        if let Some(key) = found {
            s.read_for = s.state.phase_since;
            s.missed_for = None;
            if s.character.as_deref() != Some(key.as_str()) {
                let shown = self.config.characters.get(&key).unwrap_or(&key);
                log(&format!("Character: {shown}"));
                s.character = Some(key);
            }
        } else if s.missed_for != s.state.phase_since && past_read_window(started) {
            s.missed_for = s.state.phase_since;
            win::warn("Character: Not readable this Session, using the Main Character");
        }
    }

    fn read_profile(&mut self) {
        if !self.profile_retry.due() {
            return;
        }
        let d = &mut self.derived;
        match refresh_profile(
            &self.config,
            d.family.as_deref(),
            self.game.as_ref().and_then(|g| g.service.as_deref()),
            &mut d.profile_url,
            &mut d.profile,
        ) {
            Fetch::Skipped => {}
            Fetch::Fetched => self.profile_retry.succeeded(),
            Fetch::Failed => {
                let wait = self.profile_retry.failed();
                win::warn(&format!("Profile: Retrying in {wait}s"));
            }
        }
    }

    fn check_name(&mut self) {
        let listed = |name: &String| {
            self.derived
                .profile
                .as_ref()
                .is_none_or(|p| p.characters.is_empty() || p.character(name).is_some())
        };
        let unmatched = self
            .session
            .character
            .as_ref()
            .filter(|_| self.config.identity.show_character)
            .and_then(|key| self.config.characters.get(key))
            .filter(|name| !listed(name))
            .cloned();
        if unmatched == self.derived.unmatched {
            return;
        }
        if let Some(name) = &unmatched {
            win::warn(&format!(
                "Character: {name} is not on the Profile, showing Unknown Class and Level with the Game Icon"
            ));
        }
        self.derived.unmatched = unmatched;
    }

    fn connect(&mut self) {
        match self.link.connect(&self.config.client_id) {
            Some(Ok(())) => {
                log("Discord: Connected");
                self.derived.last_sent = None;
            }
            Some(Err(e)) => win::warn(&format!("Discord: {e}")),
            None => {}
        }
    }

    fn debounce(&mut self) {
        let s = &mut self.session;
        let key = (s.state.phase, s.state.game_server.clone());
        if key != s.observed {
            s.observed = key;
            s.observed_since = Instant::now();
        }
        let first_reading = s.stable.phase.is_none() && s.state.phase.is_some();
        if first_reading || s.observed_since.elapsed().as_secs() >= self.config.debounce_seconds {
            if s.stable.phase != s.state.phase {
                log(&format!("Phase: {}", phase::label(s.state.phase)));
            }
            if s.stable.game_server != s.state.game_server {
                if let Some(key) = &s.state.game_server {
                    log(&match self.config.servers.get(key) {
                        Some(name) => format!("Server: {name}, {key}"),
                        None => format!("Server: {key}"),
                    });
                }
            }
            s.stable.phase = s.state.phase;
            s.stable.game_server = s.state.game_server.clone();
            s.stable.phase_since = s.state.phase_since;
        }
        s.stable.session_start = s.state.session_start;
    }

    fn ask_names(&mut self) {
        // Each block re-tests the prompt, so two unknown keys arriving
        // together are asked about one after the other.
        if self.config.prompt_unknown_server && !self.prompt.is_open() {
            if let Some(key) = self.session.stable.game_server.clone() {
                self.ask_name(Table::Servers, &key);
            }
        }
        if self.config.prompt_unknown_character && !self.prompt.is_open() {
            if let Some(key) = self.session.character.clone() {
                self.ask_name(Table::Characters, &key);
            }
        }
    }

    fn ask_name(&mut self, table: Table, key: &str) {
        if table.entries(&self.config).contains_key(key) || !self.prompted.insert(key.to_string()) {
            return;
        }
        let mut args = vec![crate::ui::prompt::flag(table), key];
        if let Some(service) = self.game.as_ref().and_then(|g| g.service.as_deref()) {
            args.push(service);
        }
        match self.prompt.open(&args) {
            Ok(()) => log(&format!(
                "{}: {key} is unnamed, asking for a Name",
                table.noun()
            )),
            Err(e) => win::warn(&format!("{}: Could not ask for a Name ({e})", table.noun())),
        }
    }

    fn region(&self) -> Option<String> {
        resolve_region(
            &self.config,
            self.game.as_ref().and_then(|g| g.service.as_deref()),
        )
    }

    fn push(&mut self) {
        let due = self
            .last_push
            .is_none_or(|t| t.elapsed().as_secs() >= self.config.min_update_seconds);
        let s = &self.session;
        let d = &self.derived;
        let region = self.region().unwrap_or_default();
        let ctx = context(
            &self.config,
            &s.stable,
            d.family.as_deref(),
            &region,
            s.character.as_deref(),
            d.profile.as_ref(),
        );
        let wanted = build(&self.config, &s.stable, &ctx);
        if wanted == d.last_sent || !due {
            return;
        }
        let Some(presence) = self.link.client.as_mut() else {
            return;
        };

        let result = match &wanted {
            Some(fields) => presence.set(fields),
            None => presence.clear(),
        };
        match result {
            Ok(()) => {
                log(&format!("Presence: {}", summarize(&wanted)));
                self.derived.last_sent = wanted;
                self.last_push = Some(Instant::now());
            }
            Err(e) => {
                win::warn(&format!("Discord: {e}, will reconnect"));
                self.link.lost();
                self.derived.last_sent = None;
            }
        }
    }

    fn publish(&mut self, status: Status) {
        let snapshot = self.snapshot(&status);
        if let Ok(mut shared) = self.shared.status.lock() {
            *shared = status;
        }
        if self.published.as_ref() == Some(&snapshot) {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(&self.status_path, text);
            self.published = Some(snapshot);
        }
    }

    fn status_line(&self) -> String {
        let stable = &self.session.stable;
        let mut parts = vec![phase::display(stable.phase).to_string()];
        if let Some(key) = stable.game_server.as_deref() {
            parts.push(self.config.server_name(key));
        }
        if let Some(family) = &self.derived.family {
            parts.push(family.clone());
        }
        let who = match &self.session.character {
            Some(key) => Some(
                self.config
                    .characters
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| self.config.display.unknown.clone()),
            ),
            None => self
                .derived
                .profile
                .as_ref()
                .and_then(Profile::main)
                .map(|m| m.name.clone()),
        };
        if let Some(who) = who {
            parts.push(who);
        }
        parts.join("  ·  ")
    }

    fn snapshot(&self, status: &Status) -> Snapshot {
        let row = |label: &str, value: Option<String>| value.map(|v| (label.to_string(), v));
        let group = |title: &str, rows: Vec<Option<(String, String)>>| Group {
            title: title.to_string(),
            rows: rows.into_iter().flatten().collect(),
        };
        let config = &self.config;
        let game = self.game.as_ref();
        let stable = &self.session.stable;
        let profile = self.derived.profile.as_ref();
        Snapshot {
            health: status.health,
            line: status.line.clone(),
            service: game.and_then(|g| g.service.clone()),
            placeholders: {
                let ctx = context(
                    config,
                    stable,
                    self.derived.family.as_deref(),
                    &self.region().unwrap_or_default(),
                    self.session.character.as_deref(),
                    profile,
                );
                PLACEHOLDERS
                    .iter()
                    .map(|p| (p.name.to_string(), (p.value)(&ctx).to_string()))
                    .collect()
            },
            groups: vec![
                group(
                    tr("overview.game"),
                    vec![
                        row(
                            tr("overview.folder"),
                            game.map(|g| g.root.display().to_string()),
                        ),
                        row(tr("overview.region"), game.and_then(|_| self.region())),
                        row(
                            tr("overview.log_file"),
                            game.and_then(|g| g.tail.file_name()),
                        ),
                    ],
                ),
                group(
                    tr("overview.session"),
                    vec![
                        row(
                            tr("overview.phase"),
                            Some(phase::display(stable.phase).to_string()),
                        ),
                        row(tr("overview.phase_since"), stamp(stable.phase_since)),
                        row(tr("overview.session_start"), stamp(stable.session_start)),
                        row(
                            tr("overview.server"),
                            stable.game_server.as_deref().map(|key| {
                                match config.servers.get(key) {
                                    Some(name) => {
                                        t!("overview.server_named", key = key, name = name)
                                    }
                                    None => t!("overview.server_unnamed", key = key),
                                }
                                .into_owned()
                            }),
                        ),
                    ],
                ),
                group(
                    tr("overview.you"),
                    vec![
                        row(tr("overview.family"), self.derived.family.clone()),
                        row(
                            tr("overview.character"),
                            self.session.character.as_deref().map(|key| {
                                match config.characters.get(key) {
                                    Some(name) if self.derived.unmatched.as_ref() == Some(name) => {
                                        t!(
                                            "overview.character_not_on_profile",
                                            key = key,
                                            name = name
                                        )
                                    }
                                    Some(name) => {
                                        let who = profile
                                            .and_then(|p| p.character(name))
                                            .map_or_else(|| name.clone(), describe);
                                        t!("overview.character_named", key = key, name = who)
                                    }
                                    None => t!("overview.character_unnamed", key = key),
                                }
                                .into_owned()
                            }),
                        ),
                        row(
                            tr("overview.main"),
                            profile.and_then(Profile::main).map(describe),
                        ),
                        row(
                            tr("overview.main_energy"),
                            profile.and_then(|p| p.energy.clone()),
                        ),
                        row(tr("overview.guild"), profile.and_then(|p| p.guild.clone())),
                        row(
                            tr("overview.gear_score"),
                            profile.and_then(|p| p.gear_score.clone()),
                        ),
                        row(
                            tr("overview.contribution"),
                            profile.and_then(|p| p.contribution.clone()),
                        ),
                        row(
                            tr("overview.family_created"),
                            profile.and_then(|p| p.created.clone()),
                        ),
                        row(
                            tr("overview.profile_fetched"),
                            stamp(profile.map(|p| p.fetched_at)),
                        ),
                    ],
                ),
                group(
                    tr("overview.life_skills"),
                    LIFE_SKILLS
                        .iter()
                        .map(|skill| {
                            row(
                                tr(&format!("life.{}", skill.to_lowercase())),
                                profile.and_then(|p| p.life_skills.get(*skill).cloned()),
                            )
                        })
                        .collect(),
                ),
            ],
        }
    }
}

// "Yukikiri, Deadeye, Lv. 60", or without the level when the page has none.
fn describe(character: &profile::Character) -> String {
    let class = t!(
        "overview.class",
        name = character.name,
        class = character.class
    );
    match character.level {
        Some(level) => t!("overview.class_level", who = class, level = level).into_owned(),
        None => class.into_owned(),
    }
}

pub fn stamp(unix: Option<i64>) -> Option<String> {
    let at = chrono::DateTime::from_timestamp(unix?, 0)?;
    Some(
        at.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}

fn summarize(wanted: &Option<PresenceFields>) -> String {
    let Some(fields) = wanted else {
        return "Cleared".to_string();
    };
    let details = fields.details.as_deref().unwrap_or("");
    match fields.state.as_deref() {
        Some(state) => format!("{details} | {state}"),
        None => details.to_string(),
    }
}
