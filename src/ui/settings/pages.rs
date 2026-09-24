use std::collections::BTreeSet;

use iced::widget::{
    column, container, rich_text, row, scrollable, slider, span, text, text_input, Space,
};
use iced::{border, gradient, Background, Color, Element, Length, Radians};

use super::controls::{
    dial, divider, dot, field, field_with, icon_button, marked, note, scroll_style, switch,
    switch_with,
};
use super::{check, Message, PhaseField, SettingsWindow, KEY_WIDTH};
use crate::config::{self, Table};
use crate::phase::Phase;
use crate::show::presence::PLACEHOLDERS;
use crate::ui::m3::{self, shape, type_scale};
use crate::ui::prompt;

fn avatar() -> iced::widget::image::Handle {
    static DECODED: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    DECODED
        .get_or_init(|| {
            let png = include_bytes!("../../../assets/NatsumeLS.png");
            let mut reader = png::Decoder::new(&png[..]).read_info().expect("avatar png");
            let mut pixels = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut pixels).expect("avatar pixels");
            pixels.truncate(info.buffer_size());
            // Masked here because iced clips an image to its bounds, not to a radius.
            let radius = info.width.min(info.height) as f32 / 2.0;
            for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = (i as u32 % info.width) as f32 + 0.5 - info.width as f32 / 2.0;
                let y = (i as u32 / info.width) as f32 + 0.5 - info.height as f32 / 2.0;
                let coverage = (radius - (x * x + y * y).sqrt()).clamp(0.0, 1.0);
                pixel[3] = (pixel[3] as f32 * coverage) as u8;
            }
            iced::widget::image::Handle::from_rgba(info.width, info.height, pixels)
        })
        .clone()
}

pub(super) fn about(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();

    let who = row![
        container(iced::widget::image(avatar()).width(72).height(72)).style(move |_theme| {
            container::Style::default().border(
                border::rounded(shape::FULL)
                    .color(c.outline_variant)
                    .width(1.0),
            )
        }),
        column![
            text(env!("CARGO_PKG_AUTHORS"))
                .size(type_scale::TITLE_MEDIUM)
                .color(c.on_surface),
            icon_button(
                state,
                "github.com/NatsumeLS",
                Some(Message::OpenUrl("https://github.com/NatsumeLS")),
            ),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Start),
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center);

    let mut built = column![heading(state, "Built with")].spacing(6);
    for (name, license) in LIBRARIES {
        built = built.push(pair(state, name, license.to_string()));
    }

    column![
        note(state, env!("CARGO_PKG_DESCRIPTION")),
        who,
        divider(state),
        column![
            heading(state, "This App"),
            pair(state, "Version", env!("CARGO_PKG_VERSION").to_string()),
            pair(state, "License", env!("CARGO_PKG_LICENSE").to_string()),
        ]
        .spacing(6),
        divider(state),
        built,
    ]
    .spacing(20)
    .into()
}

const LIBRARIES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/libraries.rs"));

pub(super) fn overview(state: &SettingsWindow) -> Element<'_, Message> {
    let Some(snapshot) = state.tray.snapshot.as_ref() else {
        return note(state, "The Tray is not running");
    };

    let mut page = column![].spacing(20);
    for group in &snapshot.groups {
        let mut rows = column![heading(state, &group.title)].spacing(6);
        for (label, value) in &group.rows {
            rows = rows.push(pair(state, label, value.clone()));
        }
        page = page.push(rows);
    }
    page.into()
}

pub(super) fn theme_page(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();

    let mut modes = row![].spacing(8);
    for (value, label) in [("dark", "Dark"), ("light", "Light")] {
        let selected = m3::dark(&state.config) == (value == "dark");
        modes = modes.push(m3::chip(c, label, selected, Message::ThemeMode(value)));
    }

    let chosen = m3::palette(&state.config);
    let palette_rows = wrapped(m3::Palette::ALL.map(|flavor| {
        m3::chip(
            c,
            flavor.label(),
            flavor == chosen,
            Message::ThemePalette(flavor),
        )
    }));

    let roles = [
        ("Primary", c.primary),
        ("Container", c.secondary_container),
        ("Surface", c.surface),
        ("Raised", c.surface_container),
        ("Outline", c.outline),
        ("Error", c.error),
    ];
    let mut palette = row![].spacing(10);
    for (label, color) in roles {
        palette = palette.push(
            column![
                container(Space::new().width(72).height(40)).style(move |_theme| {
                    container::background(color).border(
                        border::rounded(shape::SMALL)
                            .color(c.outline_variant)
                            .width(1.0),
                    )
                }),
                text(label)
                    .size(type_scale::BODY_MEDIUM)
                    .color(c.on_surface_variant),
            ]
            .spacing(4),
        );
    }

    column![
        marked(
            c,
            column![
                text("Mode")
                    .size(type_scale::BODY_MEDIUM)
                    .color(c.on_surface_variant),
                modes,
            ]
            .spacing(8)
            .into(),
            state.changed(lens!(theme.mode)),
        ),
        marked(
            c,
            column![
                text("Palette")
                    .size(type_scale::BODY_MEDIUM)
                    .color(c.on_surface_variant),
                palette_rows,
            ]
            .spacing(8)
            .into(),
            state.changed(lens!(theme.palette)),
        ),
        divider(state),
        field(
            state,
            "Accent Color",
            lens!(theme.accent),
            "F08080",
            check::accent,
        ),
        hue_slider(state),
        divider(state),
        column![
            text("Preview")
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
            palette,
        ]
        .spacing(8),
    ]
    .spacing(18)
    .into()
}

fn axis() -> gradient::Linear {
    gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2))
}

fn hue_slider(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let (hue, saturation, value) = state.picker;

    // Each half of the rail gets only its own slice of the wheel: iced paints
    // them as two quads, and the whole wheel on both draws it twice.
    let filled = hue / 360.0;
    let slice = |from: f32, to: f32| {
        (0..=6).fold(axis(), |rail, step| {
            let along = step as f32 / 6.0;
            rail.add_stop(
                along,
                m3::hsv_color((from + (to - from) * along) * 360.0, 1.0, 1.0),
            )
        })
    };
    let (left, right) = (slice(0.0, filled), slice(filled, 1.0));

    column![
        row![
            text("Accent Hue")
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
            Space::new().width(Length::Fill),
            text(format!("{hue:.0}°"))
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
        ],
        slider(0.0..=360.0, hue, move |h| Message::PickAccent(
            h, saturation, value
        ))
        .step(1.0_f32)
        .style(move |_theme, _status| slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Gradient(left.into()),
                    Background::Gradient(right.into()),
                ),
                width: 14.0,
                border: border::rounded(shape::FULL)
                    .color(c.outline_variant)
                    .width(1.0),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 9.0 },
                background: Background::Color(Color::WHITE),
                border_width: 2.0,
                border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            },
        }),
    ]
    .spacing(6)
    .into()
}

fn heading<'a>(state: &SettingsWindow, label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(type_scale::TITLE_MEDIUM)
        .color(state.scheme().primary)
        .into()
}

fn pair<'a>(state: &SettingsWindow, label: &'a str, value: String) -> Element<'a, Message> {
    let c = state.scheme();
    row![
        container(
            text(label)
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant)
        )
        .width(Length::Fixed(140.0)),
        text(value)
            .size(type_scale::BODY_MEDIUM)
            .color(c.on_surface),
    ]
    .spacing(12)
    .into()
}

pub(super) fn general(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        switch(state, "Enabled", lens!(enabled)),
        field(
            state,
            "Discord App ID",
            lens!(client_id),
            "From the Discord Developer Portal",
            check::app_id,
        ),
        dial(state, "Debounce Seconds", lens!(debounce_seconds), 0..=120),
        dial(
            state,
            "Minimum Seconds between Updates",
            lens!(min_update_seconds),
            5..=600,
        ),
        dial(state, "Poll Seconds", lens!(poll_seconds), 1..=60),
        switch(
            state,
            "Ask for a Name on a New Server",
            lens!(prompt_unknown_server),
        ),
        switch(
            state,
            "Ask for a Name on a New Character",
            lens!(prompt_unknown_character),
        ),
    ]
    .spacing(18)
    .into()
}

pub(super) fn identity(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        switch(state, "Show Family Name", lens!(identity.show_family)),
        switch(state, "Show Character Name", lens!(identity.show_character)),
        field(
            state,
            "Family Name",
            lens!(identity.family_name),
            "Detects from UserCache",
            |_| None,
        ),
        field(
            state,
            "Region Name",
            lens!(identity.region_name),
            "Reads from service.ini",
            |_| None,
        ),
    ]
    .spacing(18)
    .into()
}

pub(super) fn display(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        switch(state, "Show the Server", lens!(display.show_server)),
        switch(state, "Show the Region", lens!(display.show_region)),
        field(
            state,
            "Game Icon",
            lens!(display.game_icon),
            "Square PNG, JPEG, WebP or GIF URL",
            check::image,
        ),
    ]
    .spacing(18)
    .into()
}

pub(super) fn profile(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        switch(state, "Look up my Profile", lens!(profile.enabled)),
        field(
            state,
            "Profile URL",
            lens!(profile.url),
            "Searches for the Family Name",
            check::url,
        ),
        field(
            state,
            "Search URL",
            lens!(profile.search_url),
            "Region-specific",
            check::url,
        ),
        dial(
            state,
            "Refresh Minutes",
            lens!(profile.refresh_minutes),
            15..=1440,
        ),
    ]
    .spacing(18)
    .into()
}

pub(super) fn paths(state: &SettingsWindow) -> Element<'_, Message> {
    column![
        field(
            state,
            "Game Folder",
            lens!(paths.game_root),
            "Finds the running Game",
            check::game_folder,
        ),
        field(
            state,
            "User Data Folder",
            lens!(paths.user_data_dir),
            "Uses Documents/Black Desert",
            check::user_data,
        ),
    ]
    .spacing(18)
    .into()
}

pub(super) fn names(state: &SettingsWindow, table: Table) -> Element<'_, Message> {
    let c = state.scheme();
    let key_hint = match table {
        Table::Servers => "game12.sg",
        Table::Characters => "30000000000000000",
    };
    let entries = table.entries(&state.config);
    let pending = &state.pending[table as usize];

    let was = table.entries(&state.saved);
    let keys: BTreeSet<&String> = entries.keys().chain(was.keys()).collect();

    let mut rows = column![].spacing(8);
    for key in keys {
        let label = container(
            text(key.clone())
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
        )
        .width(Length::Fixed(KEY_WIDTH));

        let Some(name) = entries.get(key) else {
            let gone = was.get(key).cloned().unwrap_or_default();
            rows = rows.push(
                row![
                    dot(c, true),
                    label,
                    rich_text![span::<(), iced::Font>(gone)
                        .strikethrough(true)
                        .color(c.outline)]
                    .size(type_scale::BODY_MEDIUM)
                    .width(Length::Fill),
                    icon_button(
                        state,
                        "Restore",
                        Some(Message::TableRestore(table, key.clone()))
                    ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
            continue;
        };

        let key_for_edit = key.clone();
        let blank = check::name(name).is_some();
        rows = rows.push(
            row![
                dot(c, was.get(key) != Some(name)),
                label,
                text_input("", name)
                    .on_input(move |v| Message::TableRename(table, key_for_edit.clone(), v))
                    .size(type_scale::BODY_MEDIUM)
                    .padding([8, 12])
                    .style(move |_t, status| m3::field_style(c, status, blank)),
                icon_button(
                    state,
                    "Remove",
                    Some(Message::TableRemove(table, key.clone()))
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }

    let bad_key = match table {
        Table::Characters if !pending.0.trim().is_empty() => check::character_id(&pending.0),
        _ => None,
    };
    let wrong_key = bad_key.is_some();
    let ready = !pending.0.trim().is_empty() && !pending.1.trim().is_empty() && !wrong_key;

    let add = row![
        text_input(key_hint, &pending.0)
            .on_input(move |v| Message::TablePendingKey(table, v))
            .size(type_scale::BODY_MEDIUM)
            .padding([8, 12])
            .width(Length::Fixed(KEY_WIDTH))
            .style(move |_t, status| m3::field_style(c, status, wrong_key)),
        text_input(prompt::hint(table), &pending.1)
            .on_input(move |v| Message::TablePendingName(table, v))
            .size(type_scale::BODY_MEDIUM)
            .padding([8, 12])
            .style(move |_t, status| m3::field_style(c, status, false)),
        icon_button(state, "Add", ready.then_some(Message::TableAdd(table))),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let mut page = column![
        note(state, prompt::explain(table)),
        rows,
        divider(state),
        add
    ]
    .spacing(16);
    if let Some(reason) = bad_key {
        page = page.push(text(reason).size(type_scale::BODY_MEDIUM).color(c.error));
    }
    page.into()
}

pub(super) fn phases(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let cfg = state.config.phase(state.phase);
    let saved = state.saved.phase(state.phase);

    let chips = wrapped(Phase::ALL.map(|phase| {
        m3::chip(
            c,
            phase.label(),
            phase == state.phase,
            Message::SelectPhase(phase),
        )
    }));

    let mut fields = column![switch_with(
        state,
        "Broadcast this Phase",
        cfg.report,
        Message::PhaseReport,
        cfg.report != saved.report
    )]
    .spacing(16);
    for which in PhaseField::ALL {
        let value = which.get(&cfg);
        fields = fields.push(field_with(
            state,
            which.label(),
            value,
            which.hint(&cfg),
            move |v| Message::PhaseText(which, v),
            value != which.get(&saved),
            which.error(value),
        ));
    }

    let placeholders = PLACEHOLDERS
        .iter()
        .map(|(name, _)| format!("{{{name}}}"))
        .collect::<Vec<_>>()
        .join(" ");

    column![
        note(state, format!("Placeholders: {placeholders}")),
        note(
            state,
            "Discord shows Buttons to other People only, never to you"
        ),
        chips,
        divider(state),
        fields,
    ]
    .spacing(16)
    .into()
}

fn wrapped<'a>(chips: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    let mut rows = column![].spacing(8);
    let mut current = row![].spacing(8);
    for (index, chip) in chips.into_iter().enumerate() {
        if index > 0 && index % 4 == 0 {
            rows = rows.push(current);
            current = row![].spacing(8);
        }
        current = current.push(chip);
    }
    rows.push(current).into()
}

pub(super) fn log_page(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let body: Element<Message> = if state.log.is_empty() {
        note(state, "Nothing yet")
    } else {
        use crate::win::Level;
        // Non-breaking, so trailing spaces keep their width and every message lines up.
        let indent = "\u{A0}".repeat(Level::MESSAGE_AT);
        let mut level = Level::Info;
        let mut lines = column![];
        for line in state.log.lines() {
            let tagged = Level::of(line);
            level = tagged.unwrap_or(level);
            let (prefix, message) = match tagged {
                Some(_) => {
                    let (prefix, message) = line.split_at(Level::MESSAGE_AT.min(line.len()));
                    (prefix.replace(' ', "\u{A0}"), message)
                }
                None => (indent.clone(), line),
            };
            let color = match level {
                Level::Info => c.on_surface_variant,
                Level::Warn => c.warning,
                Level::Error => c.error,
            };
            // `Font::MONOSPACE` falls back to a face that draws `\` as `¥`.
            let mono = |s: &str| {
                text(s.to_string())
                    .size(type_scale::BODY_MEDIUM)
                    .font(iced::Font::with_name("Consolas"))
                    .color(color)
            };
            lines = lines.push(row![mono(&prefix), mono(message).width(Length::Fill)]);
        }

        container(
            scrollable(lines)
                .anchor_bottom()
                .height(Length::Fill)
                .style(move |_theme, status| scroll_style(c, status)),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fixed(380.0))
        .style(move |_t| {
            container::background(c.surface_container).border(border::rounded(shape::MEDIUM))
        })
        .into()
    };

    column![
        body,
        row![
            icon_button(state, "Open Folder", Some(Message::OpenFolder)),
            note(state, config::log_path().display().to_string()),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(16)
    .into()
}
