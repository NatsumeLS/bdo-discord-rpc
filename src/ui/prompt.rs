use std::path::PathBuf;

use iced::widget::{column, container, operation, row, text, text_input, Space};
use iced::{Element, Length, Size, Task};

use crate::config::{self, Config, Table};
use crate::ui::m3::{self, type_scale, Scheme};

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
        Table::Servers => "New Server",
        Table::Characters => "New Character",
    }
}

fn intro(table: Table, key: &str) -> String {
    match table {
        Table::Servers => format!("You are on an unnamed Server: {key}"),
        Table::Characters => format!("You are playing an unnamed Character: {key}"),
    }
}

pub fn explain(table: Table) -> &'static str {
    match table {
        Table::Servers => "An unnamed Server shows its Key.",
        Table::Characters => "An unnamed Character shows as Unknown.",
    }
}

pub fn hint(table: Table) -> &'static str {
    match table {
        Table::Servers => "Server Name",
        Table::Characters => "Character Name",
    }
}

const EDGE: u16 = 20;
const WINDOW: Size = Size::new(480.0, 232.0);

pub fn run(table: Table, key: Option<String>) -> i32 {
    let Some(key) = key else {
        let wanted = match table {
            Table::Servers => "Server Key",
            Table::Characters => "Character ID",
        };
        eprintln!("{} needs a {wanted}", flag(table));
        return 2;
    };

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

    let boot = move || {
        let state = Prompt {
            path: path.clone(),
            config: config.clone(),
            table,
            key: key.clone(),
            name: String::new(),
            error: None,
            scheme: Scheme::of(&config),
        };
        (state, operation::focus(INPUT))
    };

    let window = iced::window::Settings {
        size: WINDOW,
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

struct Prompt {
    path: PathBuf,
    config: Config,
    table: Table,
    key: String,
    name: String,
    error: Option<String>,
    scheme: Scheme,
}

impl Prompt {
    fn save_name(&mut self) -> Result<(), String> {
        // Re-read, or a Save in the settings window since this opened is lost.
        self.config = config::load_or_create(&self.path)
            .map_err(|_| "The Config File will not parse, see the Log".to_string())?;
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
    Submit,
    Dismiss,
}

fn update(state: &mut Prompt, message: Message) -> Task<Message> {
    match message {
        Message::NameChanged(name) => state.name = name,
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
        .or_else(|| blank.then(|| "A Name cannot be blank".to_string()));
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
    .spacing(14)
    .width(Length::Fill);

    if let Some(reason) = reason {
        body = body.push(text(reason).size(type_scale::BODY_MEDIUM).color(c.error));
    }

    body = body.push(
        row![
            Space::new().width(Length::Fill),
            m3::plain(c, "Skip", Some(Message::Dismiss)),
            m3::filled(c, "Save", ready.then_some(Message::Submit)),
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
