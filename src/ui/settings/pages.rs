use std::collections::BTreeSet;

use iced::widget::{
    column, container, rich_text, row, scrollable, slider, span, text, text_input, Space,
};
use iced::{border, gradient, Background, Color, Element, Length, Padding, Radians};

use super::controls::{
    dial, divider, dot, field, field_with, flow, icon_button, marked, note, switch, switch_with,
};
use super::{check, Message, PhaseField, SettingsWindow, KEY_WIDTH};
use crate::config::{self, Table};
use crate::phase::Phase;
use crate::read::profile::LIFE_SKILLS;
use crate::show::presence::PLACEHOLDERS;
use crate::ui::lang::{self, tr};
use crate::ui::m3::{self, shape, type_scale};
use crate::ui::prompt;
use rust_i18n::t;

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
                Some(Message::OpenUrl("https://github.com/NatsumeLS".into())),
            ),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Start),
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center);

    let mut built = column![heading(state, tr("about.built_with"))].spacing(6);
    for (name, license) in LIBRARIES {
        built = built.push(pair(state, name, license.to_string()));
    }

    column![
        note(state, env!("CARGO_PKG_DESCRIPTION")),
        who,
        divider(state),
        column![
            heading(state, tr("about.this_app")),
            pair(
                state,
                tr("about.version"),
                env!("CARGO_PKG_VERSION").to_string()
            ),
            pair(
                state,
                tr("about.license"),
                env!("CARGO_PKG_LICENSE").to_string()
            ),
        ]
        .spacing(6),
        divider(state),
        built,
    ]
    .spacing(20)
    .into()
}

const LIBRARIES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/libraries.rs"));

pub(super) fn placeholders(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let values = state.tray.snapshot.as_ref().map(|s| &s.placeholders);
    let meaning = |name: &str| match LIFE_SKILLS
        .iter()
        .find(|skill| skill.eq_ignore_ascii_case(name))
    {
        Some(_) => t!(
            "placeholders.life_skill",
            skill = tr(&format!("life.{name}"))
        )
        .into_owned(),
        None => tr(&format!("placeholders.{name}")).to_string(),
    };

    let mut page = column![note(
        state,
        tr(match values {
            Some(_) => "placeholders.note",
            None => "placeholders.note_no_tray",
        })
    )]
    .spacing(20);
    for (group, members) in PLACEHOLDERS
        .chunk_by(|a, b| a.group == b.group)
        .map(|run| (run[0].group, run))
    {
        let mut rows =
            column![heading(state, tr(&format!("placeholders.group_{group}")))].spacing(6);
        for placeholder in members {
            let value = values
                .and_then(|v| v.get(placeholder.name))
                .cloned()
                .unwrap_or_default();
            rows = rows.push(
                row![
                    container(
                        text(format!("{{{}}}", placeholder.name))
                            .size(type_scale::BODY_MEDIUM)
                            .color(c.primary)
                    )
                    .width(Length::Fixed(200.0)),
                    container(
                        text(meaning(placeholder.name))
                            .size(type_scale::BODY_MEDIUM)
                            .color(c.on_surface_variant)
                    )
                    .width(Length::Fixed(220.0)),
                    text(value)
                        .size(type_scale::BODY_MEDIUM)
                        .color(c.on_surface)
                        .width(Length::Fill),
                ]
                .spacing(12),
            );
        }
        page = page.push(rows);
    }
    page.into()
}

pub(super) fn overview(state: &SettingsWindow) -> Element<'_, Message> {
    let Some(snapshot) = state.tray.snapshot.as_ref() else {
        return note(state, tr("overview.tray_not_running"));
    };

    let mut groups = Vec::new();
    for group in snapshot.groups.iter().filter(|g| !g.rows.is_empty()) {
        let mut rows = column![heading(state, &group.title)].spacing(6);
        for (label, value) in &group.rows {
            rows = rows.push(pair(state, label, value.clone()));
        }
        let height = GROUP_HEADING + PAIR_ROW * group.rows.len() as f32;
        groups.push((height, rows.into()));
    }
    flow(state, groups, 20.0)
}

pub(super) fn theme_page(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();

    let mut modes = row![].spacing(8);
    for (value, label) in [("dark", tr("theme.dark")), ("light", tr("theme.light"))] {
        let selected = m3::dark(&state.config) == (value == "dark");
        modes = modes.push(m3::chip(c, label, selected, Message::ThemeMode(value)));
    }

    let chosen = m3::palette(&state.config);
    let palette_rows = wrapped(m3::Palette::ALL.map(|flavor| {
        m3::chip(
            c,
            tr(&format!("palette.{}", flavor.key())),
            flavor == chosen,
            Message::ThemePalette(flavor),
        )
    }));

    let roles = [
        (tr("theme.role_primary"), c.primary),
        (tr("theme.role_container"), c.secondary_container),
        (tr("theme.role_surface"), c.surface),
        (tr("theme.role_raised"), c.surface_container),
        (tr("theme.role_outline"), c.outline),
        (tr("theme.role_error"), c.error),
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
                text(tr("theme.mode"))
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
                text(tr("theme.palette"))
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
            tr("theme.accent"),
            lens!(theme.accent),
            "F08080",
            check::accent,
        ),
        hue_slider(state),
        divider(state),
        column![
            text(tr("theme.preview"))
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
            text(tr("theme.hue"))
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

fn language(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let current = state.config.language.trim();
    let choices = std::iter::once((lang::AUTO.to_string(), tr("language.automatic"))).chain(
        lang::available().into_iter().map(|code| {
            let name = lang::name(&code);
            (code, name)
        }),
    );

    let mut chips = row![].spacing(8);
    for (code, label) in choices {
        let selected = current == code || (current.is_empty() && code == lang::AUTO);
        chips = chips.push(m3::chip(
            c,
            label,
            selected,
            Message::Text(lens!(language), code),
        ));
    }

    marked(
        c,
        column![
            text(tr("general.language"))
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
            chips,
        ]
        .spacing(8)
        .into(),
        state.changed(lens!(language)),
    )
}

// Estimated heights for `flow`: a switch, and a labeled field, slider or chip row.
const LINE: f32 = 24.0;
const BLOCK: f32 = 72.0;

// A blank field shows the value it would use, or its hint when nothing was found.
fn detected<'a>(
    state: &'a SettingsWindow,
    value: fn(&crate::watch::Detected) -> &Option<String>,
    hint: &'a str,
) -> &'a str {
    state
        .tray
        .snapshot
        .as_ref()
        .and_then(|s| value(&s.detected).as_deref())
        .unwrap_or(hint)
}
// One label and value row on Overview and the heading above a group of them.
const PAIR_ROW: f32 = 24.0;
const GROUP_HEADING: f32 = 28.0;
// The two notes, the phase chips and the divider above the Phases fields.
const PHASES_TOP: f32 = 200.0;

pub(super) fn general(state: &SettingsWindow) -> Element<'_, Message> {
    flow(
        state,
        vec![
            (LINE, switch(state, tr("general.enabled"), lens!(enabled))),
            (BLOCK, language(state)),
            (
                BLOCK,
                field(
                    state,
                    tr("general.app_id"),
                    lens!(client_id),
                    tr("general.app_id_hint"),
                    check::app_id,
                ),
            ),
            (
                BLOCK,
                dial(
                    state,
                    tr("general.debounce"),
                    lens!(debounce_seconds),
                    0..=60,
                ),
            ),
            (
                BLOCK,
                dial(state, tr("general.poll"), lens!(poll_seconds), 1..=60),
            ),
            (
                LINE,
                switch(
                    state,
                    tr("general.ask_server"),
                    lens!(prompt_unknown_server),
                ),
            ),
            (
                LINE,
                switch(
                    state,
                    tr("general.ask_character"),
                    lens!(prompt_unknown_character),
                ),
            ),
        ],
        18.0,
    )
}

pub(super) fn identity(state: &SettingsWindow) -> Element<'_, Message> {
    flow(
        state,
        vec![
            (
                LINE,
                switch(
                    state,
                    tr("identity.show_family"),
                    lens!(identity.show_family),
                ),
            ),
            (
                LINE,
                switch(
                    state,
                    tr("identity.show_character"),
                    lens!(identity.show_character),
                ),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("identity.family"),
                    lens!(identity.family_name),
                    detected(state, |d| &d.family, tr("identity.family_hint")),
                    |_| None,
                ),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("identity.region"),
                    lens!(identity.region_name),
                    detected(state, |d| &d.region, tr("identity.region_hint")),
                    |_| None,
                ),
            ),
        ],
        18.0,
    )
}

pub(super) fn display(state: &SettingsWindow) -> Element<'_, Message> {
    flow(
        state,
        vec![
            (
                LINE,
                switch(state, tr("display.show_server"), lens!(display.show_server)),
            ),
            (
                LINE,
                switch(state, tr("display.show_region"), lens!(display.show_region)),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("display.game_icon"),
                    lens!(display.game_icon),
                    tr("display.game_icon_hint"),
                    check::image,
                ),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("display.unknown"),
                    lens!(display.unknown),
                    tr("display.unknown_hint"),
                    |_| None,
                ),
            ),
        ],
        18.0,
    )
}

pub(super) fn profile(state: &SettingsWindow) -> Element<'_, Message> {
    flow(
        state,
        vec![
            (
                LINE,
                switch(state, tr("profile.enabled"), lens!(profile.enabled)),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("profile.url"),
                    lens!(profile.url),
                    detected(state, |d| &d.profile_url, tr("profile.url_hint")),
                    check::url,
                ),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("profile.search"),
                    lens!(profile.search_url),
                    detected(state, |d| &d.search_url, tr("profile.search_hint")),
                    check::url,
                ),
            ),
            (
                BLOCK,
                dial(
                    state,
                    tr("profile.refresh"),
                    lens!(profile.refresh_minutes),
                    15..=1440,
                ),
            ),
        ],
        18.0,
    )
}

pub(super) fn paths(state: &SettingsWindow) -> Element<'_, Message> {
    flow(
        state,
        vec![
            (
                BLOCK,
                field(
                    state,
                    tr("paths.game"),
                    lens!(paths.game_root),
                    detected(state, |d| &d.game_root, tr("paths.game_hint")),
                    check::game_folder,
                ),
            ),
            (
                BLOCK,
                field(
                    state,
                    tr("paths.user_data"),
                    lens!(paths.user_data_dir),
                    detected(state, |d| &d.user_data_dir, tr("paths.user_data_hint")),
                    check::user_data,
                ),
            ),
        ],
        18.0,
    )
}

pub(super) fn names(state: &SettingsWindow, table: Table) -> Element<'_, Message> {
    let c = state.scheme();
    let key_hint = match table {
        Table::Servers => tr("names.server_key"),
        Table::Characters => tr("names.character_key"),
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
                        tr("names.restore"),
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
                m3::clearable(
                    c,
                    text_input("", name)
                        .on_input(move |v| Message::TableRename(table, key_for_edit.clone(), v))
                        .size(type_scale::BODY_MEDIUM)
                        .padding(Padding::from([8, 12]).right(12.0 + m3::CLEAR_ROOM))
                        .style(move |_t, status| m3::field_style(c, status, blank)),
                    (!name.is_empty()).then(|| Message::TableRename(
                        table,
                        key.clone(),
                        String::new()
                    )),
                ),
                icon_button(
                    state,
                    tr("names.remove"),
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
        m3::clearable(
            c,
            text_input(key_hint, &pending.0)
                .on_input(move |v| Message::TablePendingKey(table, v))
                .size(type_scale::BODY_MEDIUM)
                .padding(Padding::from([8, 12]).right(12.0 + m3::CLEAR_ROOM))
                .width(Length::Fixed(KEY_WIDTH))
                .style(move |_t, status| m3::field_style(c, status, wrong_key)),
            (!pending.0.is_empty()).then(|| Message::TablePendingKey(table, String::new())),
        ),
        m3::clearable(
            c,
            text_input(prompt::hint(table), &pending.1)
                .on_input(move |v| Message::TablePendingName(table, v))
                .size(type_scale::BODY_MEDIUM)
                .padding(Padding::from([8, 12]).right(12.0 + m3::CLEAR_ROOM))
                .style(move |_t, status| m3::field_style(c, status, false)),
            (!pending.1.is_empty()).then(|| Message::TablePendingName(table, String::new())),
        ),
        icon_button(
            state,
            tr("names.add"),
            ready.then_some(Message::TableAdd(table))
        ),
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
    let snapshot = state.tray.snapshot.as_ref();
    let seen: Vec<&String> = match table {
        Table::Servers => snapshot
            .and_then(|s| s.server_key.as_ref())
            .into_iter()
            .collect(),
        Table::Characters => snapshot
            .map(|s| s.character_ids.iter().collect())
            .unwrap_or_default(),
    };
    page = page.push(prompt::picker(
        c,
        seen.into_iter().filter(|key| !entries.contains_key(*key)),
        &pending.0,
        move |key| Message::TablePendingKey(table, key),
    ));
    let known = match table {
        Table::Servers => crate::region::servers(snapshot.and_then(|s| s.service.as_deref())),
        Table::Characters => state.characters.iter().collect(),
    };
    page.push(prompt::picker(
        c,
        prompt::untaken(known, table, &state.config),
        &pending.1,
        move |name| Message::TablePendingName(table, name),
    ))
    .into()
}

pub(super) fn phases(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    let cfg = state.config.phase(state.phase);
    let saved = state.saved.phase(state.phase);

    let chips = wrapped(Phase::ALL.map(|phase| {
        m3::chip(
            c,
            crate::phase::display(Some(phase)),
            phase == state.phase,
            Message::SelectPhase(phase),
        )
    }));

    let mut fields = column![switch_with(
        state,
        tr("phases.broadcast"),
        cfg.report,
        Message::PhaseReport,
        cfg.report != saved.report
    )]
    .spacing(16);
    // The fields come in pairs (an image and its hover, a label and its URL),
    // so when one column runs out of height each pair shares a row instead.
    let one_column = PHASES_TOP + LINE + (BLOCK + 16.0) * PhaseField::ALL.len() as f32;
    let per_row = if state.two_columns(one_column) { 2 } else { 1 };
    for pair in PhaseField::ALL.chunks(per_row) {
        let mut line = row![].spacing(super::COLUMN_GAP);
        for &which in pair {
            let value = which.get(&cfg);
            line = line.push(
                container(field_with(
                    state,
                    which.label(),
                    value,
                    which.hint(&cfg),
                    move |v| Message::PhaseText(which, v),
                    value != which.get(&saved),
                    which.error(value),
                ))
                .width(Length::Fill),
            );
        }
        fields = fields.push(line);
    }

    column![
        note(state, tr("phases.placeholders")),
        note(state, tr("phases.buttons_note")),
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
        note(state, tr("log.empty"))
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
                .style(move |_theme, status| m3::scroll_style(c, status)),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fixed(state.log_height()))
        .style(move |_t| {
            container::background(c.surface_container).border(border::rounded(shape::MEDIUM))
        })
        .into()
    };

    column![
        body,
        row![
            icon_button(state, tr("log.open_folder"), Some(Message::OpenFolder)),
            icon_button(
                state,
                tr("log.copy"),
                (!state.log.is_empty()).then_some(Message::CopyLog)
            ),
            note(state, config::log_path().display().to_string()),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(16)
    .into()
}
