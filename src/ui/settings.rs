use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{border, Color, Element, Length, Size, Task};

use crate::config::{self, Config, PhaseConfig, Table};
use crate::phase::Phase;
use crate::ui::lang::{self, tr};
use crate::ui::m3::{self, shape, type_scale, Scheme};
use crate::ui::tray::Health;
use crate::watch;
use rust_i18n::t;

#[derive(Debug)]
pub struct Lens<T: 'static> {
    get: fn(&Config) -> &T,
    get_mut: fn(&mut Config) -> &mut T,
}

// By hand, since deriving would demand `T: Copy` and rule out `String`.
impl<T> Clone for Lens<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Lens<T> {}

macro_rules! lens {
    ($($field:ident).+) => {
        crate::ui::settings::Lens {
            get: |c| &c.$($field).+,
            get_mut: |c| &mut c.$($field).+,
        }
    };
}

mod check;
mod controls;
mod pages;

use controls::dot;

const WINDOW: Size = Size::new(960.0, 800.0);
const RAIL_WIDTH: f32 = 232.0;
const EDGE: u16 = 24;
const KEY_WIDTH: f32 = 180.0;
const REFRESH: Duration = Duration::from_secs(1);
const LOG_TAIL_BYTES: u64 = 64 * 1024;

pub fn run(page: Page) -> i32 {
    if !crate::win::claim_instance(crate::win::SETTINGS) {
        crate::win::focus_window(crate::ui::tray::TOOLTIP);
        return 0;
    }

    let path = config::config_path();
    let (config, unreadable) = match config::load_or_create(&path) {
        Ok(config) => (config, false),
        Err(e) => {
            eprintln!("Config Error: {e}");
            (Config::default(), true)
        }
    };
    let start = if unreadable { Page::Log } else { page };
    lang::apply(&config);

    let boot = move || {
        let state = SettingsWindow {
            path: path.clone(),
            saved: config.clone(),
            config: config.clone(),
            unreadable,
            no_baseline: unreadable,
            page: start,
            message: None,
            phase: Phase::Play,
            pending: Default::default(),
            characters: crate::ui::prompt::character_names(),
            log: String::new(),
            config_mtime: config::mtime(&path),
            tray: Tray::default(),
            picker: m3::hsv_of(m3::seed(&config)),
            picker_accent: config.theme.accent.clone(),
        };
        (state, refresh_later())
    };

    let window = iced::window::Settings {
        size: WINDOW,
        min_size: Some(Size::new(760.0, 520.0)),
        position: iced::window::Position::Centered,
        icon: crate::ui::tray::window_icon(),
        ..iced::window::Settings::default()
    };

    match iced::application(boot, update, view)
        .title(|_: &SettingsWindow| crate::ui::tray::TOOLTIP.to_string())
        .window(window)
        .theme(|state: &SettingsWindow| m3::theme(&state.config))
        .run()
    {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Could not open the Settings Window: {e}");
            1
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    General,
    Identity,
    Display,
    Theme,
    Profile,
    Paths,
    Servers,
    Characters,
    Phases,
    Log,
    Placeholders,
    About,
}

impl Page {
    const SECTIONS: [(&str, &[Page]); 4] = [
        ("rail.status", &[Page::Overview, Page::Log]),
        (
            "rail.presence",
            &[Page::Identity, Page::Display, Page::Phases, Page::Profile],
        ),
        ("rail.names", &[Page::Servers, Page::Characters]),
        (
            "rail.app",
            &[
                Page::General,
                Page::Theme,
                Page::Paths,
                Page::Placeholders,
                Page::About,
            ],
        ),
    ];

    fn dirty(self, config: &Config, saved: &Config, no_baseline: bool) -> bool {
        match self {
            Page::Overview | Page::Log | Page::Placeholders | Page::About => false,
            _ if no_baseline => true,
            Page::General => {
                config.enabled != saved.enabled
                    || config.language != saved.language
                    || config.client_id != saved.client_id
                    || config.debounce_seconds != saved.debounce_seconds
                    || config.min_update_seconds != saved.min_update_seconds
                    || config.poll_seconds != saved.poll_seconds
                    || config.prompt_unknown_server != saved.prompt_unknown_server
                    || config.prompt_unknown_character != saved.prompt_unknown_character
            }
            Page::Identity => config.identity != saved.identity,
            Page::Display => config.display != saved.display,
            Page::Theme => config.theme != saved.theme,
            Page::Profile => config.profile != saved.profile,
            Page::Paths => config.paths != saved.paths,
            Page::Servers => config.servers != saved.servers,
            Page::Characters => config.characters != saved.characters,
            Page::Phases => config.phases != saved.phases,
        }
    }

    fn label(self) -> &'static str {
        tr(match self {
            Page::Overview => "page.overview",
            Page::General => "page.general",
            Page::Identity => "page.identity",
            Page::Display => "page.display",
            Page::Theme => "page.theme",
            Page::Profile => "page.profile",
            Page::Paths => "page.paths",
            Page::Servers => "page.servers",
            Page::Characters => "page.characters",
            Page::Phases => "page.phases",
            Page::Log => "page.log",
            Page::Placeholders => "page.placeholders",
            Page::About => "page.about",
        })
    }
}

#[derive(Default)]
struct Tray {
    snapshot: Option<watch::Snapshot>,
}

impl Tray {
    fn refresh(&mut self) {
        self.snapshot = if crate::win::tray_running() {
            watch::Snapshot::read(&config::status_path()).or(self.snapshot.take())
        } else {
            None
        };
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Save,
    Discard,
    Reset,
}

struct SettingsWindow {
    path: PathBuf,
    config: Config,
    saved: Config,
    page: Page,
    message: Option<(String, bool)>,
    phase: Phase,
    pending: [(String, String); 2],
    characters: Vec<String>,
    log: String,
    config_mtime: Option<SystemTime>,
    unreadable: bool,
    no_baseline: bool,
    tray: Tray,
    picker: (f32, f32, f32),
    picker_accent: String,
}

#[derive(Debug, Clone, Copy)]
enum PhaseField {
    Details,
    State,
    LargeImage,
    LargeText,
    SmallImage,
    SmallText,
    ButtonLabel,
    ButtonUrl,
    SecondButtonLabel,
    SecondButtonUrl,
}

impl PhaseField {
    const ALL: [PhaseField; 10] = [
        PhaseField::Details,
        PhaseField::State,
        PhaseField::LargeImage,
        PhaseField::LargeText,
        PhaseField::SmallImage,
        PhaseField::SmallText,
        PhaseField::ButtonLabel,
        PhaseField::ButtonUrl,
        PhaseField::SecondButtonLabel,
        PhaseField::SecondButtonUrl,
    ];

    fn label(self) -> &'static str {
        tr(match self {
            PhaseField::Details => "phases.details",
            PhaseField::State => "phases.state",
            PhaseField::LargeImage => "phases.large_image",
            PhaseField::LargeText => "phases.large_text",
            PhaseField::SmallImage => "phases.small_image",
            PhaseField::SmallText => "phases.small_text",
            PhaseField::ButtonLabel => "phases.button_label",
            PhaseField::ButtonUrl => "phases.button_url",
            PhaseField::SecondButtonLabel => "phases.second_button_label",
            PhaseField::SecondButtonUrl => "phases.second_button_url",
        })
    }

    fn hint(self, cfg: &PhaseConfig) -> &'static str {
        match self {
            PhaseField::Details => tr("phases.hint_details"),
            PhaseField::State => tr("phases.hint_state"),
            PhaseField::LargeImage => tr("phases.hint_game_icon"),
            PhaseField::SmallImage if cfg.large_image.trim().is_empty() => {
                tr("phases.hint_no_image")
            }
            PhaseField::SmallImage => tr("phases.hint_game_icon"),
            PhaseField::LargeText => "",
            PhaseField::SmallText => tr("phases.hint_small_text"),
            PhaseField::ButtonLabel
            | PhaseField::ButtonUrl
            | PhaseField::SecondButtonLabel
            | PhaseField::SecondButtonUrl => tr("phases.hint_no_button"),
        }
    }

    fn error(self, value: &str) -> Option<String> {
        check::template(value).or_else(|| match self {
            PhaseField::LargeImage | PhaseField::SmallImage => check::image(value),
            PhaseField::ButtonLabel | PhaseField::SecondButtonLabel => check::button_label(value),
            PhaseField::ButtonUrl | PhaseField::SecondButtonUrl => check::button_url(value),
            _ => None,
        })
    }

    fn get(self, cfg: &PhaseConfig) -> &String {
        match self {
            PhaseField::Details => &cfg.details,
            PhaseField::State => &cfg.state,
            PhaseField::LargeImage => &cfg.large_image,
            PhaseField::LargeText => &cfg.large_text,
            PhaseField::SmallImage => &cfg.small_image,
            PhaseField::SmallText => &cfg.small_text,
            PhaseField::ButtonLabel => &cfg.button_label,
            PhaseField::ButtonUrl => &cfg.button_url,
            PhaseField::SecondButtonLabel => &cfg.second_button_label,
            PhaseField::SecondButtonUrl => &cfg.second_button_url,
        }
    }

    fn set(self, cfg: &mut PhaseConfig, value: String) {
        match self {
            PhaseField::Details => cfg.details = value,
            PhaseField::State => cfg.state = value,
            PhaseField::LargeImage => cfg.large_image = value,
            PhaseField::LargeText => cfg.large_text = value,
            PhaseField::SmallImage => cfg.small_image = value,
            PhaseField::SmallText => cfg.small_text = value,
            PhaseField::ButtonLabel => cfg.button_label = value,
            PhaseField::ButtonUrl => cfg.button_url = value,
            PhaseField::SecondButtonLabel => cfg.second_button_label = value,
            PhaseField::SecondButtonUrl => cfg.second_button_url = value,
        }
    }
}

impl SettingsWindow {
    fn scheme(&self) -> Scheme {
        Scheme::of(&self.config)
    }

    fn error(&self) -> Option<String> {
        let c = &self.config;
        let named = |label: &str, reason: Option<String>| {
            reason.map(|r| t!("actions.field_error", field = label, reason = r).into_owned())
        };

        if self.unreadable {
            return Some(tr("actions.unreadable").to_string());
        }

        named(tr("theme.accent"), check::accent(&c.theme.accent))
            .or_else(|| named(tr("general.app_id"), check::app_id(&c.client_id)))
            .or_else(|| named(tr("display.game_icon"), check::image(&c.display.game_icon)))
            .or_else(|| named(tr("profile.url"), check::url(&c.profile.url)))
            .or_else(|| named(tr("profile.search"), check::url(&c.profile.search_url)))
            .or_else(|| named(tr("paths.game"), check::game_folder(&c.paths.game_root)))
            .or_else(|| {
                named(
                    tr("paths.user_data"),
                    check::user_data(&c.paths.user_data_dir),
                )
            })
            .or_else(|| {
                Phase::ALL.into_iter().find_map(|p| {
                    let cfg = c.phase(p);
                    PhaseField::ALL.into_iter().find_map(|field| {
                        named(
                            &t!(
                                "phases.field_label",
                                phase = crate::phase::display(Some(p)),
                                field = field.label()
                            ),
                            field.error(field.get(&cfg)),
                        )
                    })
                })
            })
            .or_else(|| {
                c.servers
                    .iter()
                    .chain(c.characters.iter())
                    .find_map(|(key, name)| named(key, check::name(name)))
            })
            .or_else(|| {
                c.characters
                    .keys()
                    .find_map(|key| named(key, check::character_id(key)))
            })
    }

    fn edit_phase(&mut self, edit: impl FnOnce(&mut PhaseConfig)) {
        let mut cfg = self.config.phase(self.phase);
        edit(&mut cfg);
        self.config.phases.insert(self.phase.key().to_string(), cfg);
    }

    fn dirty(&self) -> bool {
        self.no_baseline || self.config != self.saved
    }

    fn changed<T: PartialEq>(&self, lens: Lens<T>) -> bool {
        (lens.get)(&self.config) != (lens.get)(&self.saved)
    }

    fn save(&mut self) {
        let kept = self
            .no_baseline
            .then(|| self.path.with_extension("toml.broken"));
        if let Some(kept) = &kept {
            if self.path.exists() && std::fs::rename(&self.path, kept).is_err() {
                self.message = Some((tr("actions.set_aside_failed").into(), false));
                return;
            }
        }

        self.message = match config::save(&self.path, &self.config) {
            Ok(()) => {
                crate::win::signal_config_changed();
                self.saved = self.config.clone();
                self.no_baseline = false;
                self.config_mtime = config::mtime(&self.path);
                match kept.as_ref().and_then(|p| p.file_name()) {
                    Some(name) => Some((
                        t!("actions.saved_kept", file = name.to_string_lossy()).into_owned(),
                        true,
                    )),
                    None => Some((tr("actions.saved").into(), true)),
                }
            }
            Err(e) => Some((e, false)),
        };
    }

    fn refresh(&mut self) {
        self.log = read_log_tail(&config::log_path());
        self.tray.refresh();

        let current = config::mtime(&self.path);
        if current.is_none() || current == self.config_mtime {
            return;
        }
        self.config_mtime = current;

        let Ok(disk) = config::load_or_create(&self.path) else {
            self.unreadable = true;
            self.no_baseline = true;
            self.page = Page::Log;
            return;
        };
        self.unreadable = false;
        self.no_baseline = false;

        if !self.dirty() {
            self.saved = disk.clone();
            self.config = disk;
            return;
        }

        // Before `saved` is replaced, or a row removed on screen comes back.
        merge_new_names(&mut self.config.servers, &self.saved.servers, &disk.servers);
        merge_new_names(
            &mut self.config.characters,
            &self.saved.characters,
            &disk.characters,
        );
        self.saved = disk;
        self.message = Some((tr("actions.changed_on_disk").into(), false));
    }
}

fn read_log_tail(path: &Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let Ok(end) = file.seek(SeekFrom::End(0)) else {
        return String::new();
    };
    let start = end.saturating_sub(LOG_TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }

    let mut buffer = Vec::new();
    if file.read_to_end(&mut buffer).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buffer);

    match text.find('\n') {
        Some(newline) if start > 0 => text[newline + 1..].to_string(),
        _ => text.into_owned(),
    }
}

fn merge_new_names(
    editing: &mut BTreeMap<String, String>,
    saved: &BTreeMap<String, String>,
    disk: &BTreeMap<String, String>,
) {
    for (key, name) in disk {
        if !saved.contains_key(key) && !editing.contains_key(key) {
            editing.insert(key.clone(), name.clone());
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(Page),
    Act(Action),
    Bool(Lens<bool>, bool),
    Text(Lens<String>, String),
    Number(Lens<u64>, u64),
    TableRename(Table, String, String),
    TableRemove(Table, String),
    TableRestore(Table, String),
    TablePendingKey(Table, String),
    TablePendingName(Table, String),
    TableAdd(Table),
    SelectPhase(Phase),
    PhaseText(PhaseField, String),
    PhaseReport(bool),
    OpenFolder,
    OpenUrl(&'static str),
    ThemeMode(&'static str),
    ThemePalette(m3::Palette),
    PickAccent(f32, f32, f32),
    Refresh,
}

fn refresh_later() -> Task<Message> {
    Task::future(async {
        // Blocking on purpose: `iced::time::every` needs an async runtime.
        std::thread::sleep(REFRESH);
        Message::Refresh
    })
}

fn update(state: &mut SettingsWindow, message: Message) -> Task<Message> {
    let from_picker = matches!(message, Message::PickAccent(..));
    let mut task = Task::none();

    match message {
        Message::Navigate(page) => state.page = page,
        Message::Act(action) => match action {
            Action::Save => state.save(),
            Action::Discard => {
                state.config = state.saved.clone();
                state.message = None;
            }
            Action::Reset => {
                state.config = Config::default();
                state.unreadable = false;
            }
        },
        Message::Bool(lens, v) => *(lens.get_mut)(&mut state.config) = v,
        Message::Text(lens, v) => *(lens.get_mut)(&mut state.config) = v,
        Message::Number(lens, v) => *(lens.get_mut)(&mut state.config) = v,
        Message::TableRename(table, key, name) => {
            if let Some(slot) = table.entries_mut(&mut state.config).get_mut(&key) {
                *slot = name;
            }
        }
        Message::TableRemove(table, key) => {
            table.entries_mut(&mut state.config).remove(&key);
        }
        Message::TableRestore(table, key) => {
            if let Some(name) = table.entries(&state.saved).get(&key).cloned() {
                table.entries_mut(&mut state.config).insert(key, name);
            }
        }
        Message::TablePendingKey(table, key) => state.pending[table as usize].0 = key,
        Message::TablePendingName(table, name) => state.pending[table as usize].1 = name,
        Message::TableAdd(table) => {
            let (key, name) = &state.pending[table as usize];
            let (key, name) = (key.trim().to_string(), name.trim().to_string());
            if !key.is_empty() && !name.is_empty() {
                table.entries_mut(&mut state.config).insert(key, name);
                state.pending[table as usize] = Default::default();
            }
        }
        Message::SelectPhase(phase) => state.phase = phase,
        Message::PhaseText(field, value) => state.edit_phase(|cfg| field.set(cfg, value)),
        Message::PhaseReport(report) => state.edit_phase(|cfg| cfg.report = report),
        Message::OpenFolder => {
            // Explorer ignores a quoted `/select,`, which `arg` adds for a path with spaces.
            let _ = std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", config::log_path().display()))
                .spawn();
        }
        Message::OpenUrl(url) => {
            let _ = std::process::Command::new("explorer").arg(url).spawn();
        }
        Message::ThemeMode(mode) => {
            state.config.theme.mode = mode.to_string();
        }
        Message::ThemePalette(palette) => {
            state.config.theme.palette = palette.key().to_string();
        }
        Message::PickAccent(hue, saturation, value) => {
            state.picker = (hue, saturation, value);
            state.config.theme.accent = m3::hsv_hex(hue, saturation, value);
        }
        Message::Refresh => {
            state.refresh();
            task = refresh_later();
        }
    }

    // Re-sync only when the accent moved from elsewhere, or black and gray,
    // which have no hue to read back, would snap the slider to red.
    if !from_picker && state.config.theme.accent != state.picker_accent {
        state.picker = m3::hsv_of(m3::seed(&state.config));
    }
    state.picker_accent.clone_from(&state.config.theme.accent);
    lang::apply(&state.config);
    task
}

fn view(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        row![rail(state), page(state)].height(Length::Fill),
        status_bar(state),
    ]
    .into()
}

fn rail(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let mut items = column![].spacing(4).padding(12);

    let sections: &[(&str, &[Page])] = if state.unreadable {
        &[("", &[Page::Log])]
    } else {
        &Page::SECTIONS
    };

    for (index, (title, pages)) in sections.iter().enumerate() {
        if !title.is_empty() {
            items = items.push(
                container(
                    text(tr(title))
                        .size(type_scale::LABEL_LARGE)
                        .color(c.primary),
                )
                .padding(iced::Padding {
                    top: if index == 0 { 4.0 } else { 8.0 },
                    right: 20.0,
                    bottom: 4.0,
                    left: 20.0,
                }),
            );
        }
        for &page in *pages {
            items = items.push(rail_item(state, page));
        }
    }

    container(
        scrollable(items)
            .height(Length::Fill)
            .style(move |_theme, status| m3::scroll_style(c, status)),
    )
    .width(Length::Fixed(RAIL_WIDTH))
    .height(Length::Fill)
    .style(move |_theme| container::background(c.surface_container))
    .into()
}

fn rail_item(state: &SettingsWindow, page: Page) -> Element<'_, Message> {
    let c = state.scheme();
    let selected = page == state.page;
    let marker = dot(
        c,
        page.dirty(&state.config, &state.saved, state.no_baseline),
    );

    button(
        row![
            text(page.label())
                .size(type_scale::LABEL_LARGE)
                .width(Length::Fill),
            marker,
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding([12, 20])
    .on_press(Message::Navigate(page))
    .style(move |_theme, status| {
        let (base, fg) = if selected {
            (c.secondary_container, c.on_secondary_container)
        } else {
            (Color::TRANSPARENT, c.on_surface_variant)
        };
        m3::pill(m3::mix(base, c.on_surface, m3::layer(status)), fg)
    })
    .into()
}

fn page(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();

    let heading = text(state.page.label())
        .size(type_scale::HEADLINE_SMALL)
        .color(c.on_surface);

    let body: Element<Message> = match state.page {
        Page::Overview => pages::overview(state),
        Page::General => pages::general(state),
        Page::Identity => pages::identity(state),
        Page::Display => pages::display(state),
        Page::Theme => pages::theme_page(state),
        Page::Profile => pages::profile(state),
        Page::Paths => pages::paths(state),
        Page::Servers => pages::names(state, Table::Servers),
        Page::Characters => pages::names(state, Table::Characters),
        Page::Phases => pages::phases(state),
        Page::Log => pages::log_page(state),
        Page::Placeholders => pages::placeholders(state),
        Page::About => pages::about(state),
    };

    let content = column![heading, Space::new().height(8), body]
        .spacing(4)
        .width(Length::Fill);

    container(
        column![
            scrollable(container(content).padding(EDGE))
                .height(Length::Fill)
                .style(move |_theme, status| m3::scroll_style(c, status)),
            actions(state),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| container::background(c.surface))
    .into()
}

fn status_bar(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();

    let (dot, line) = match state.tray.snapshot.as_ref() {
        Some(snapshot) => (
            match snapshot.health {
                Health::Idle => c.outline,
                Health::Waiting => c.on_surface_variant,
                Health::Live => c.primary,
            },
            snapshot.line.clone(),
        ),
        None => (c.outline, tr("status.tray_not_running").to_string()),
    };

    container(
        row![
            container(Space::new().width(8).height(8)).style(move |_theme| {
                container::background(dot).border(border::rounded(shape::FULL))
            }),
            text(line)
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding([8, EDGE])
    .style(move |_theme| container::background(c.surface_container_high))
    .into()
}

fn actions(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let dirty = state.dirty();
    let error = state.error();

    let status: Element<Message> = match (error.clone(), dirty, &state.message) {
        (Some(reason), _, _) => text(reason)
            .size(type_scale::BODY_MEDIUM)
            .color(c.error)
            .into(),
        (None, true, _) => text(tr("actions.unsaved"))
            .size(type_scale::BODY_MEDIUM)
            .color(c.error)
            .into(),
        (None, false, Some((body, ok))) => text(body.clone())
            .size(type_scale::BODY_MEDIUM)
            .color(if *ok { c.primary } else { c.error })
            .into(),
        _ => Space::new().into(),
    };

    let mut buttons = row![].spacing(8).align_y(iced::Alignment::Center);
    if !state.unreadable {
        buttons = buttons.push(m3::plain(
            c,
            tr("actions.discard"),
            dirty.then_some(Message::Act(Action::Discard)),
        ));
    }
    buttons = buttons.push(m3::filled(
        c,
        tr("actions.save"),
        (dirty && error.is_none()).then_some(Message::Act(Action::Save)),
    ));

    container(
        row![
            container(status).width(Length::Fill),
            m3::plain(c, tr("actions.reset"), Some(Message::Act(Action::Reset))),
            buttons,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding([12, EDGE])
    .style(move |_theme| container::background(c.surface_container))
    .into()
}
