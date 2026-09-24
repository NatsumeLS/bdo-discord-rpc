use iced::widget::{
    button, column, container, row, slider, text, text_input, toggler, Column, Space,
};
use iced::{border, Background, Color, Element, Length, Padding};

use super::{Lens, Message, SettingsWindow};
use crate::ui::m3::{self, shape, type_scale, Scheme};

pub(super) fn dot(c: Scheme, showing: bool) -> Element<'static, Message> {
    let color = if showing {
        c.primary
    } else {
        Color::TRANSPARENT
    };
    // A fixed size: iced has no stretch alignment, so `Length::Fill` here
    // would lay out at zero height and paint nothing.
    container(Space::new().width(6).height(6))
        .style(move |_theme| container::background(color).border(border::rounded(shape::FULL)))
        .into()
}

pub(super) fn marked<'a>(
    c: Scheme,
    control: Element<'a, Message>,
    dirty: bool,
) -> Element<'a, Message> {
    if !dirty {
        return control;
    }
    row![dot(c, true), control]
        .spacing(10)
        .align_y(iced::Alignment::Start)
        .into()
}

// One column, or two once the window is wide. Items keep their reading order,
// down the left column and then the right, split where half the weight falls.
pub(super) fn flow<'a>(
    state: &SettingsWindow,
    items: Vec<(usize, Element<'a, Message>)>,
    spacing: f32,
) -> Element<'a, Message> {
    if !state.wide() {
        return Column::with_children(items.into_iter().map(|(_, item)| item))
            .spacing(spacing)
            .into();
    }
    let total: usize = items.iter().map(|(weight, _)| weight).sum();
    let (mut left, mut right) = (column![].spacing(spacing), column![].spacing(spacing));
    let mut placed = 0;
    for (weight, item) in items {
        if placed * 2 < total {
            left = left.push(item);
            placed += weight;
        } else {
            right = right.push(item);
        }
    }
    row![left.width(Length::Fill), right.width(Length::Fill)]
        .spacing(super::COLUMN_GAP)
        .into()
}

pub(super) fn divider(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(move |_t| container::background(c.outline_variant))
        .into()
}

pub(super) fn icon_button<'a>(
    state: &SettingsWindow,
    label: &'a str,
    message: Option<Message>,
) -> Element<'a, Message> {
    let c = state.scheme();
    button(text(label).size(type_scale::LABEL_LARGE))
        .padding([8, 16])
        .on_press_maybe(message)
        .style(move |_t, status| {
            let (fg, outline) = if matches!(status, button::Status::Disabled) {
                (c.on_surface_variant, c.outline_variant)
            } else {
                (c.primary, c.outline)
            };
            button::Style {
                border: border::rounded(shape::FULL).color(outline).width(1.0),
                ..m3::pill(
                    Color {
                        a: m3::layer(status),
                        ..c.primary
                    },
                    fg,
                )
            }
        })
        .into()
}

pub(super) fn note<'a>(
    state: &SettingsWindow,
    body: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    let body = body.into_fragment();
    let lines = body
        .split_inclusive(". ")
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    text(lines)
        .size(type_scale::BODY_MEDIUM)
        .color(state.scheme().on_surface_variant)
        .into()
}

pub(super) fn switch<'a>(
    state: &SettingsWindow,
    label: &'a str,
    lens: Lens<bool>,
) -> Element<'a, Message> {
    switch_with(
        state,
        label,
        *(lens.get)(&state.config),
        move |v| Message::Bool(lens, v),
        state.changed(lens),
    )
}

pub(super) fn switch_with<'a>(
    state: &SettingsWindow,
    label: &'a str,
    value: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
    dirty: bool,
) -> Element<'a, Message> {
    let c = state.scheme();
    let control = toggler(value)
        .label(label)
        .text_size(type_scale::BODY_LARGE)
        .on_toggle(on_toggle)
        .style(move |_theme, _status| toggler::Style {
            background: Background::Color(if value {
                c.primary
            } else {
                c.surface_container_high
            }),
            background_border_width: if value { 0.0 } else { 2.0 },
            background_border_color: c.outline,
            foreground: Background::Color(if value { c.on_primary } else { c.outline }),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: Some(c.on_surface),
            border_radius: Some(shape::FULL.into()),
            padding_ratio: 0.22,
        });
    marked(c, control.into(), dirty)
}

pub(super) fn field<'a>(
    state: &SettingsWindow,
    label: &'a str,
    lens: Lens<String>,
    placeholder: &'a str,
    check: fn(&str) -> Option<String>,
) -> Element<'a, Message> {
    let value = (lens.get)(&state.config);
    field_with(
        state,
        label,
        value,
        placeholder,
        move |v| Message::Text(lens, v),
        state.changed(lens),
        check(value),
    )
}

pub(super) fn field_with<'a>(
    state: &SettingsWindow,
    label: &'a str,
    value: &str,
    placeholder: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    dirty: bool,
    error: Option<String>,
) -> Element<'a, Message> {
    let c = state.scheme();
    let wrong = error.is_some();
    let clear = (!value.is_empty()).then(|| on_input(String::new()));
    let input = text_input(placeholder, value)
        .on_input(on_input)
        .size(type_scale::BODY_LARGE)
        .padding(Padding::from([12, 16]).right(16.0 + m3::CLEAR_ROOM))
        .style(move |_theme, status| m3::field_style(c, status, wrong));
    let mut control = column![
        text(label).size(type_scale::BODY_MEDIUM).color(if wrong {
            c.error
        } else {
            c.on_surface_variant
        }),
        m3::clearable(c, input, clear),
    ]
    .spacing(6);
    if let Some(reason) = error {
        control = control.push(text(reason).size(type_scale::BODY_MEDIUM).color(c.error));
    }
    marked(c, control.into(), dirty)
}

pub(super) fn dial<'a>(
    state: &SettingsWindow,
    label: &'a str,
    lens: Lens<u64>,
    range: std::ops::RangeInclusive<u64>,
) -> Element<'a, Message> {
    let c = state.scheme();
    let value = *(lens.get)(&state.config);
    let control = column![
        row![
            text(label)
                .size(type_scale::BODY_MEDIUM)
                .color(c.on_surface_variant),
            Space::new().width(Length::Fill),
            text(value.to_string())
                .size(type_scale::BODY_MEDIUM)
                .color(c.primary),
        ],
        // u32, since the slider needs `f64: From<T>` and u64 has none.
        slider(
            *range.start() as u32..=*range.end() as u32,
            value as u32,
            move |v| Message::Number(lens, u64::from(v)),
        )
        .style(move |_theme, _status| slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(c.primary),
                    Background::Color(c.surface_container_high),
                ),
                width: 6.0,
                border: border::rounded(shape::FULL),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Rectangle {
                    width: 4,
                    border_radius: shape::FULL.into(),
                },
                background: Background::Color(c.primary),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }),
    ]
    .spacing(6);
    marked(c, control.into(), state.changed(lens))
}
