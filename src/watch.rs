use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Local;

use crate::config::{self, Config, Table};
use crate::phase::{self, Phase};
use crate::read::capture::nodes::Place;
use crate::read::capture::{CaptureState, Captured};
use crate::read::game::{self, GameFinder, GameProcess};
use crate::read::log_tail::{GameState, LogTail};
use crate::read::profile::{self, Profile};
use crate::region;
use crate::show::discord::Presence;
use crate::show::presence::{self, build, context, Context, PresenceFields};
use crate::ui::lang::tr;
use crate::ui::tray::Health;
use crate::win::{self, log};

mod snapshot;

pub use snapshot::{Detected, Snapshot};

const POLL: Duration = Duration::from_secs(1);
const DEBOUNCE: Duration = Duration::from_secs(3);
const PROFILE_MAX_AGE: i64 = 60 * 60;

const CONNECT_MIN_BACKOFF: u64 = 5;
const CONNECT_MAX_BACKOFF: u64 = 60;

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
    /// The settings window's status bar.
    pub line: String,
    /// The tray's hover text, the phase alone once live.
    pub tooltip: String,
}

impl Status {
    fn new(health: Health, line: impl Into<String>) -> Self {
        let line = line.into();
        Status {
            health,
            tooltip: line.clone(),
            line,
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::new(Health::Idle, tr("status.starting_up"))
    }
}

#[derive(Default)]
pub struct Shared {
    pub status: Mutex<Status>,
    pub quit: AtomicBool,
    pub config_error: AtomicBool,
    pub captured: Mutex<Captured>,
    pub capture: Mutex<CaptureState>,
    /// What the server hosts end in, like `sg.pearl-bdo.com`, from the log.
    pub server_domain: Mutex<Option<String>>,
}

// The configured folder replaces the root only: the process is what says the
// game is running and which log is this session's.
pub fn resolve_game(config: &Config, finder: &mut GameFinder) -> Option<GameProcess> {
    let mut process = finder.find()?;
    if let Some(root) = non_empty(&config.paths.game_root) {
        process.root = root.into();
    }
    Some(process)
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
        // A cache for another family or another URL would stand in for an hour.
        *profile = profile::load_cache(&cache_path).filter(|cached| {
            match non_empty(&config.profile.url) {
                Some(wanted) => cached.url.as_deref() == Some(wanted.as_str()),
                None => family
                    .zip(cached.family.as_deref())
                    .is_none_or(|(wanted, found)| wanted.eq_ignore_ascii_case(found)),
            }
        });
        if let Some(cached) = profile.as_ref() {
            log(&format!(
                "Profile: Loaded from the Cache, fetched {}",
                stamp(Some(cached.fetched_at)).unwrap_or_default()
            ));
        }
    }

    if profile
        .as_ref()
        .is_some_and(|p| Local::now().timestamp() - p.fetched_at < PROFILE_MAX_AGE)
    {
        return Fetch::Skipped;
    }

    let target = match url {
        Some(url) => url.clone(),
        None => {
            let found = if let Some(configured) = non_empty(&config.profile.url) {
                configured
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

    // A search with no exact match takes its first result, which may be
    // another family's page.
    let searched = non_empty(&config.profile.url).is_none();
    let fetched =
        profile::fetch(&target).and_then(|fetched| match (family, fetched.family.as_deref()) {
            (Some(wanted), Some(found)) if searched && !wanted.eq_ignore_ascii_case(found) => {
                *url = None;
                Err(format!("Found the Page of {found}, not {wanted}"))
            }
            _ => Ok(fetched),
        });
    match fetched {
        Ok(fetched) => {
            match fetched.main() {
                Some(main) => log(&format!(
                    "Profile: Fetched {} Characters, Main {}",
                    fetched.characters.len(),
                    main.summary()
                )),
                None => win::warn("Profile: Fetched, but it lists no Characters"),
            }
            if fetched.hidden {
                win::warn(&format!(
                    "Profile: Levels, Guild and Stats are hidden (sign in, open {target} and change its Privacy Settings)"
                ));
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
    family_missing: bool,
    profile: Option<Profile>,
    profile_url: Option<String>,
    unmatched: Option<String>,
    last_sent: Option<PresenceFields>,
    user_cache: UserCache,
}

// What the UserCache folders say, rescanned when the game comes or goes or
// the phase changes, the moments the client writes them.
#[derive(Default)]
struct UserCache {
    scanned_for: Option<(bool, Option<Phase>)>,
    families: usize,
    newest_family: Option<String>,
    character_ids: Vec<String>,
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
    started_at: Option<SystemTime>,
    tail: LogTail,
    service: Option<String>,
    version: Option<u32>,
    cap_warned: bool,
    /// The read error last warned about, so each is said once.
    unreadable: Option<String>,
}

struct Session {
    state: GameState,
    stable: GameState,
    observed: (Option<Phase>, Option<String>),
    observed_since: Instant,
    character: Option<String>,
    read_for: Option<i64>,
    missed_for: Option<i64>,
    capture_missed_for: Option<i64>,
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
            capture_missed_for: None,
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
        watcher.reload_asked = win::sleep_until_config_changes(POLL);
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
    profile_retry: Backoff,

    families: usize,

    prompted: HashSet<String>,
    prompt: win::ChildWindow,

    status_path: PathBuf,
    published: Option<Snapshot>,
    status_failed: bool,
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
            profile_retry: Backoff::new(PROFILE_MIN_BACKOFF, PROFILE_MAX_BACKOFF),
            families: 0,
            prompted: HashSet::new(),
            prompt: win::ChildWindow::default(),
            status_path: config::status_path(),
            published: None,
            status_failed: false,
        }
    }

    fn tick(&mut self) -> Status {
        self.reload();
        self.scan_user_cache();

        if !self.loaded {
            return Status::new(Health::Waiting, tr("status.config_error"));
        }

        if !self.config.enabled {
            if !self.disabled {
                self.disabled = true;
                log("Config: Disabled, standing by");
                // Not reset(): the retry schedule survives being switched off.
                self.link.client = None;
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
        if game.tail.capped && !game.cap_warned {
            game.cap_warned = true;
            win::warn(
                "Log File: The Game stopped writing it at its Size Cap, so it has nothing new until the Game restarts",
            );
        }
        if game.tail.error != game.unreadable {
            if let Some(e) = &game.tail.error {
                win::warn(&format!("Log File: Could not read it ({e})"));
            }
            game.unreadable.clone_from(&game.tail.error);
        }
        self.follow_capture();
        self.check_capture();

        self.count_families();
        self.read_family();
        self.read_character();
        self.read_profile();
        self.check_name();
        self.connect();
        self.debounce();
        self.ask_names();
        self.push();

        match self.link.client {
            Some(_) => Status {
                tooltip: phase::display(self.session.stable.phase),
                ..Status::new(Health::Live, self.status_line())
            },
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
        // The start time too, or a restart within one poll keeps the old session.
        if running.as_ref().map(|p| (&p.root, p.started_at))
            == self.game.as_ref().map(|g| (&g.root, g.started_at))
        {
            return;
        }

        self.game = match running {
            Some(process) => {
                log(&format!("Game: Found at {}", process.root.display()));
                let version = game::client_version(&process.root);
                if let Some(version) = version {
                    log(&format!("Game: Client Version {version}"));
                }
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
                    version,
                    root: process.root,
                    started_at: process.started_at,
                    cap_warned: false,
                    unreadable: None,
                })
            }
            None => {
                log("Game: Closed, clearing the Presence");
                self.link.reset();
                self.derived.last_sent = None;
                None
            }
        };
        self.session = Session::default();
    }

    fn scan_user_cache(&mut self) {
        let key = (self.game.is_some(), self.session.stable.phase);
        let cache = &mut self.derived.user_cache;
        if cache.scanned_for == Some(key) {
            return;
        }
        let dir = resolve_user_data_dir(&self.config);
        *cache = UserCache {
            scanned_for: Some(key),
            families: dir.as_deref().map_or(0, game::family_count),
            newest_family: dir.as_deref().and_then(game::detect_family),
            character_ids: dir.as_deref().map(game::character_ids).unwrap_or_default(),
        };
    }

    fn count_families(&mut self) {
        let families = self.derived.user_cache.families;
        if families > 1 && self.families <= 1 && !self.family_chosen() {
            win::warn(&format!(
                "Family: {families} Accounts in UserCache, using the most recent (set the Family Name to choose one)"
            ));
        }
        self.families = families;
    }

    // Read even when hidden, since the profile lookup searches by it.
    fn read_family(&mut self) {
        let d = &mut self.derived;
        if d.family.is_some() {
            return;
        }
        d.family = d.user_cache.newest_family.clone();
        match &d.family {
            Some(name) => log(&format!("Family: {name}")),
            None if !d.family_missing => {
                d.family_missing = true;
                win::warn("Family: Not found in UserCache, retrying");
            }
            None => {}
        }
    }

    // The fallback for when the capture has not named the character.
    fn read_character(&mut self) {
        let from_capture = self.captured_raw().character.is_some();
        let s = &mut self.session;
        if from_capture || !self.config.identity.show_character || s.state.phase_since == s.read_for
        {
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
            // Or the character before the swap stands in, not the main.
            s.character = None;
            win::warn("Character: Not readable, using the Main Character until the next Load-in");
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
            .and_then(|key| self.character_name(key))
            .filter(|name| !listed(name));
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
        if first_reading || s.observed_since.elapsed() >= DEBOUNCE {
            if s.stable.phase != s.state.phase {
                log(&format!("Phase: {}", phase::label(s.state.phase)));
            }
            if s.stable.game_server != s.state.game_server {
                if let Some(key) = &s.state.game_server {
                    let name = self.config.server_name(key);
                    log(&if name == *key {
                        format!("Server: {key}")
                    } else {
                        format!("Server: {name}, {key}")
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
            // The capture knows its name in game, so there is nothing to ask.
            if let Some(key) = self.session.character.clone() {
                if self.character_name(&key).is_none() {
                    self.ask_name(Table::Characters, &key);
                }
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

    /// The capture names the world server by its address, which keeps working
    /// after the client log reaches its size cap and stops, and catches the
    /// Magnus, which the log does not name as a server change.
    fn follow_capture(&mut self) {
        let domain = self
            .session
            .state
            .game_host
            .as_deref()
            .and_then(|host| host.split_once('.'))
            .map(|(_, domain)| domain.to_string());
        if let (Some(domain), Ok(mut shared)) = (domain, self.shared.server_domain.lock()) {
            *shared = Some(domain);
        }
        let captured = self.captured_raw();
        let s = &mut self.session;

        // On a world server not named yet, the log's may be one left long
        // ago, so none is better than that.
        if captured.server.is_some() || captured.resolving {
            s.state.game_server = captured.server.clone();
        }
        // Whichever saw a change last wins: the log still has the finer
        // phases before entering the world, and stops at its size cap.
        if let Some((phase, since)) = captured.phase {
            if s.state.phase_since.is_none_or(|logged| since >= logged) {
                s.state.phase = Some(phase);
                s.state.phase_since = Some(since);
            }
        }
        if let Some((id, name)) = &captured.character {
            if self.config.identity.show_character && s.character.as_ref() != Some(id) {
                log(&format!(
                    "Character: {}",
                    self.config.characters.get(id).unwrap_or(name)
                ));
                s.character = Some(id.clone());
                s.missed_for = None;
            }
        }
        if let Some(family) = &captured.family {
            if !self.family_chosen() && self.derived.family.as_ref() != Some(family) {
                log(&format!("Family: {family}"));
                self.derived.family = Some(family.clone());
            }
        }
        // Into Character Names, so the prompt is left for what the capture
        // cannot name. Marked as offered, so a failed save is not retried.
        if let Some((id, name)) = captured.character {
            if !name.is_empty()
                && !self.config.characters.contains_key(&id)
                && self.prompted.insert(id.clone())
            {
                self.save_character_name(&id, &name);
            }
        }
    }

    fn save_character_name(&mut self, id: &str, name: &str) {
        match config::add_name(&self.config_path, Table::Characters, id, name) {
            Ok(()) => {
                log(&format!("Character: Saved {name} as the Name of {id}"));
                self.config
                    .characters
                    .insert(id.to_string(), name.to_string());
                // Its own write is no edit to reload for.
                self.config_mtime = config::mtime(&self.config_path);
            }
            Err(e) => win::warn(&format!("Character: Could not save the Name of {id} ({e})")),
        }
    }

    /// A running capture that sees the log enter the world and no entry of
    /// its own has stopped decoding, most likely after a patch moved the
    /// messages it reads, so the files take over without it saying so.
    fn check_capture(&mut self) {
        let listening = match self.capture_state() {
            CaptureState::Listening(since) => since,
            _ => 0,
        };
        // Held from the entry until its connection ends, so the log's In Game
        // after the post-entry loading screen still counts it.
        let entered = self.captured_raw().character.is_some();
        let s = &mut self.session;
        let (Some(Phase::Play), Some(since)) = (s.state.phase, s.state.phase_since) else {
            return;
        };
        let window = game::CHARACTER_READ_WINDOW.as_secs() as i64;
        let missed = listening != 0
            && since > listening
            && Local::now().timestamp() - since >= window
            && !entered;
        if missed && s.capture_missed_for != Some(since) {
            s.capture_missed_for = Some(since);
            win::warn("Capture: Did not see the Character enter, falling back to the Game Files");
        }
    }

    /// How the capture is doing, as the key the Overview row and the status
    /// line each word their own way.
    fn capture_kind(&self) -> &'static str {
        let s = &self.session;
        let missed = s.capture_missed_for.is_some() && s.capture_missed_for == s.state.phase_since;
        let captured = self.captured_raw();
        let waiting = captured.joined && captured.entered.is_none();
        match self.capture_state() {
            CaptureState::Starting => "starting",
            CaptureState::NoNpcap => "no_npcap",
            CaptureState::Failed(_) => "failed",
            CaptureState::Listening(_) if missed => "missed",
            CaptureState::Listening(_) if waiting => "waiting",
            CaptureState::Listening(_) => "listening",
        }
    }

    fn capture_state(&self) -> CaptureState {
        self.shared
            .capture
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    fn captured_raw(&self) -> Captured {
        self.shared
            .captured
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// The capture's view for the presence, with the place made safe to show.
    fn captured(&self) -> Captured {
        let mut captured = self.captured_raw();
        if self.session.state.phase != Some(Phase::Play) {
            captured.place = Place::default();
        }
        captured
    }

    fn character_name(&self, key: &str) -> Option<String> {
        presence::character_name(&self.config, &self.captured_raw(), key)
    }

    fn service(&self) -> Option<&str> {
        self.game.as_ref().and_then(|g| g.service.as_deref())
    }

    fn family_chosen(&self) -> bool {
        non_empty(&self.config.identity.family_name).is_some()
    }

    fn region(&self) -> Option<String> {
        resolve_region(&self.config, self.service())
    }

    fn presence_context(&self) -> Context {
        context(
            &self.config,
            &self.session.stable,
            self.derived.family.as_deref(),
            &self.region().unwrap_or_default(),
            self.session.character.as_deref(),
            self.derived.profile.as_ref(),
            &self.captured(),
        )
    }

    fn push(&mut self) {
        let wanted = build(&self.config, &self.session.stable, &self.presence_context());
        if wanted == self.derived.last_sent {
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
        // Only once written, so a failed write is tried again next tick.
        if let Ok(text) = serde_json::to_string(&snapshot) {
            match config::write_atomic(&self.status_path, &text) {
                Ok(()) => {
                    self.published = Some(snapshot);
                    self.status_failed = false;
                }
                Err(e) if !self.status_failed => {
                    self.status_failed = true;
                    win::warn(&format!(
                        "Status: Could not write it for the Settings Window, retrying ({e})"
                    ));
                }
                Err(_) => {}
            }
        }
    }

    /// The tray's tooltip and the settings window's status bar: the phase,
    /// then how the two sources behind it are doing.
    fn status_line(&self) -> String {
        let log = match self
            .game
            .as_ref()
            .map(|g| (g.tail.file_name(), g.tail.capped))
        {
            Some((_, true)) => "capped",
            Some((Some(_), false)) => "reading",
            _ => "none",
        };
        [
            phase::display(self.session.stable.phase).to_string(),
            tr(&format!("status.capture_{}", self.capture_kind())).to_string(),
            tr(&format!("status.log_{log}")).to_string(),
        ]
        .join("  ·  ")
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
