use std::path::PathBuf;

use iced::widget::{column, container, operation, row, scrollable, text, text_input, Space};
use iced::{Element, Length, Size, Task};

use crate::config::{self, Config, Table};
use crate::read::profile;
use crate::region;
use crate::ui::lang::{self, tr};
use crate::ui::m3::{self, type_scale, Scheme};
use rust_i18n::t;

pub fn flag(table: Table) -> &'static str {
    match table {
        Table::Servers => "--name-server",
        Table::Characters => "--name-character",
    }
}

pub fn from_flag(arg: &str) -> Option<Table> {
    [Table::Servers, Table::Characters]
        .into_iter()
        .find(|&table| flag(table) == arg)
}

fn title(table: Table) -> &'static str {
    match table {
        Table::Servers => tr("prompt.new_server"),
        Table::Characters => tr("prompt.new_character"),
    }
}

fn intro(table: Table, key: &str) -> String {
    match table {
        Table::Servers => t!("prompt.unnamed_server", key = key),
        Table::Characters => t!("prompt.unnamed_character", key = key),
    }
    .into_owned()
}

pub fn explain(table: Table) -> &'static str {
    match table {
        Table::Servers => tr("names.server_explain"),
        Table::Characters => tr("names.character_explain"),
    }
}

pub fn hint(table: Table) -> &'static str {
    match table {
        Table::Servers => tr("names.server_name"),
        Table::Characters => tr("names.character_name"),
    }
}

const EDGE: u16 = 20;
const WINDOW: Size = Size::new(480.0, 232.0);
const PICKER: f32 = 80.0;

pub fn character_names() -> Vec<String> {
    profile::load_cache(&config::profile_cache_path())
        .map(|cached| cached.characters.into_iter().map(|c| c.name).collect())
        .unwrap_or_default()
}

pub fn untaken<'a>(
    known: impl IntoIterator<Item = &'a String> + 'a,
    table: Table,
    config: &'a Config,
) -> impl Iterator<Item = &'a String> {
    let taken = table.entries(config);
    known
        .into_iter()
        .filter(move |name| !taken.values().any(|used| used == *name))
}

pub fn picker<'a, M: Clone + 'a>(
    c: Scheme,
    names: impl IntoIterator<Item = &'a String>,
    typed: &str,
    pick: impl Fn(String) -> M,
) -> Element<'a, M> {
    let typed = typed.trim();
    let lowered = typed.to_lowercase();
    names
        .into_iter()
        .filter(|name| name.to_lowercase().contains(&lowered))
        .fold(row![].spacing(8), |chips, name| {
            chips.push(m3::chip(c, name, name == typed, pick(name.clone())))
        })
        .wrap()
        .vertical_spacing(8)
        .into()
}

pub fn run(table: Table, key: Option<String>, service: Option<String>) -> i32 {
    let Some(key) = key else {
        let wanted = match table {
            Table::Servers => "Server Key",
            Table::Characters => "Character ID",
        };
        eprintln!("{} needs a {wanted}", flag(table));
        return 2;
    };

    // Before the titles are looked up, since the running prompt is found by its title.
    if let Ok(config) = config::load_or_create(&config::config_path()) {
        lang::apply(&config);
    }

    if !crate::win::claim_instance(crate::win::PROMPT) {
        crate::win::focus_window(title(Table::Servers));
        crate::win::focus_window(title(Table::Characters));
        return 0;
    }

    if table == Table::Characters && (key.is_empty() || !key.chars().all(|c| c.is_ascii_digit())) {
        eprintln!("A Character ID is all digits, got {key:?}");
        return 2;
    }

    let path = config::config_path();
    let config = match config::load_or_create(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Config Error: {e}");
            return 1;
        }
    };

    let characters = character_names();
    let known = match table {
        Table::Servers => region::servers(service.as_deref()),
        Table::Characters => characters.iter().collect(),
    };
    let suggestions: Vec<String> = untaken(known, table, &config).cloned().collect();
    let size = if suggestions.is_empty() {
        WINDOW
    } else {
        Size::new(WINDOW.width, WINDOW.height + PICKER + SPACING)
    };

    let boot = move || {
        let state = Prompt {
            path: path.clone(),
            config: config.clone(),
            table,
            key: key.clone(),
            name: String::new(),
            suggestions: suggestions.clone(),
            error: None,
            scheme: Scheme::of(&config),
        };
        (state, operation::focus(INPUT))
    };

    let window = iced::window::Settings {
        size,
        resizable: false,
        position: iced::window::Position::Centered,
        icon: crate::ui::tray::window_icon(),
        ..iced::window::Settings::default()
    };

    match iced::application(boot, update, view)
        .title(title(table))
        .window(window)
        .theme(|state: &Prompt| m3::theme(&state.config))
        .run()
    {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Could not open the {} Prompt: {e}", title(table));
            1
        }
    }
}

const INPUT: &str = "name";
const SPACING: f32 = 14.0;

struct Prompt {
    path: PathBuf,
    config: Config,
    table: Table,
    key: String,
    name: String,
    suggestions: Vec<String>,
    error: Option<String>,
    scheme: Scheme,
}

impl Prompt {
    fn save_name(&mut self) -> Result<(), String> {
        // Re-read, or a Save in the settings window since this opened is lost.
        self.config =
            config::load_or_create(&self.path).map_err(|_| tr("prompt.unreadable").to_string())?;
        self.table
            .entries_mut(&mut self.config)
            .insert(self.key.clone(), self.name.trim().to_string());
        config::save(&self.path, &self.config)?;
        crate::win::signal_config_changed();
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum Message {
    NameChanged(String),
    Pick(String),
    Submit,
    Dismiss,
}

fn update(state: &mut Prompt, message: Message) -> Task<Message> {
    match message {
        Message::NameChanged(name) | Message::Pick(name) => state.name = name,
        Message::Submit => {
            if state.name.trim().is_empty() {
                return Task::none();
            }
            return match state.save_name() {
                Ok(()) => iced::exit(),
                Err(e) => {
                    state.error = Some(e);
                    Task::none()
                }
            };
        }
        Message::Dismiss => return iced::exit(),
    }
    Task::none()
}

fn view(state: &Prompt) -> Element<'_, Message> {
    let c = state.scheme;
    let ready = !state.name.trim().is_empty();

    let blank = !state.name.is_empty() && state.name.trim().is_empty();
    let reason = state
        .error
        .clone()
        .or_else(|| blank.then(|| tr("prompt.blank").to_string()));
    let wrong = reason.is_some();

    let field = text_input(hint(state.table), &state.name)
        .id(INPUT)
        .on_input(Message::NameChanged)
        .on_submit(Message::Submit)
        .size(type_scale::BODY_LARGE)
        .padding([12, 16])
        .style(move |_theme, status| m3::field_style(c, status, wrong));

    let mut body = column![
        text(intro(state.table, &state.key))
            .size(type_scale::TITLE_MEDIUM)
            .color(c.on_surface),
        text(explain(state.table))
            .size(type_scale::BODY_MEDIUM)
            .color(c.on_surface_variant),
        field,
    ]
    .spacing(SPACING)
    .width(Length::Fill);

    if let Some(reason) = reason {
        body = body.push(text(reason).size(type_scale::BODY_MEDIUM).color(c.error));
    }

    if !state.suggestions.is_empty() {
        let chips = picker(c, &state.suggestions, &state.name, Message::Pick);
        body = body.push(
            scrollable(chips)
                .height(PICKER)
                .style(move |_theme, status| m3::scroll_style(c, status)),
        );
    }

    body = body.push(
        row![
            Space::new().width(Length::Fill),
            m3::plain(c, tr("prompt.skip"), Some(Message::Dismiss)),
            m3::filled(c, tr("prompt.save"), ready.then_some(Message::Submit)),
        ]
        .spacing(8),
    );

    container(body)
        .padding(EDGE)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::background(c.surface))
        .into()
}
