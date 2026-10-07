use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{layout, mouse, overlay, renderer, Clipboard, Layout, Shell};
use iced::widget::text::IntoFragment;
use iced::widget::{column, container, row, slider, text, text_input, toggler, Space};
use iced::{border, Background, Color, Element, Event, Length, Padding, Point, Rectangle, Size};

use super::{Lens, Message, SettingsWindow, COLUMN_GAP, COLUMN_WIDTH};
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
    // Always a row, or the first edit swaps the widget and a field loses focus.
    row![dot(c, dirty), control]
        .spacing(10)
        .align_y(iced::Alignment::Start)
        .into()
}

// One column, or two once one would run past `fit` and the width holds two.
// Heights are measured at layout, not estimated. Down splits the cells where
// half the height falls, Across pairs them on rows, and a head spans both.
pub(super) struct Flow<'a> {
    children: Vec<Element<'a, Message>>,
    head: bool,
    across: bool,
    spacing: f32,
    fit: f32,
}

impl<'a> Flow<'a> {
    pub(super) fn new(fit: f32, cells: Vec<Element<'a, Message>>) -> Self {
        Flow {
            children: cells,
            head: false,
            across: false,
            spacing: 18.0,
            fit,
        }
    }

    pub(super) fn head(mut self, head: Element<'a, Message>) -> Self {
        self.children.insert(0, head);
        self.head = true;
        self
    }

    pub(super) fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub(super) fn across(mut self) -> Self {
        self.across = true;
        self
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Flow<'_> {
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let half = (width - COLUMN_GAP) / 2.0;
        let spacing = self.spacing;
        let skip = usize::from(self.head);
        let mut measure = |widths: &dyn Fn(usize) -> f32| -> Vec<layout::Node> {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .enumerate()
                .map(|(index, (child, tree))| {
                    let limits =
                        layout::Limits::new(Size::ZERO, Size::new(widths(index), f32::INFINITY));
                    child.as_widget_mut().layout(tree, renderer, &limits)
                })
                .collect()
        };
        let stack = |nodes: &mut [layout::Node], x: f32, mut y: f32| {
            for node in nodes {
                node.move_to_mut(Point::new(x, y));
                y += node.size().height + spacing;
            }
            y
        };

        let mut nodes = measure(&|_| width);
        let one = stack(&mut nodes, 0.0, 0.0) - spacing;
        if one <= self.fit || half < COLUMN_WIDTH {
            return layout::Node::with_children(Size::new(width, one.max(0.0)), nodes);
        }

        let mut nodes = measure(&|index| if index < skip { width } else { half });
        let top = stack(&mut nodes[..skip], 0.0, 0.0);
        let cells = &mut nodes[skip..];
        let bottom = if self.across {
            let mut y = top;
            for pair in cells.chunks_mut(2) {
                let mut tallest: f32 = 0.0;
                for (side, node) in pair.iter_mut().enumerate() {
                    node.move_to_mut(Point::new(side as f32 * (half + COLUMN_GAP), y));
                    tallest = tallest.max(node.size().height);
                }
                y += tallest + spacing;
            }
            y
        } else {
            let total: f32 = cells.iter().map(|n| n.size().height + spacing).sum();
            let mut placed = 0.0;
            let split = cells
                .iter()
                .take_while(|node| {
                    let fits = placed * 2.0 < total;
                    placed += node.size().height + spacing;
                    fits
                })
                .count();
            let (left, right) = cells.split_at_mut(split);
            stack(left, 0.0, top).max(stack(right, half + COLUMN_GAP, top))
        };
        layout::Node::with_children(Size::new(width, (bottom - spacing).max(0.0)), nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((child, state), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(state, layout, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((child, state), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                state, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((child, state), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .filter(|(_, layout)| layout.bounds().intersects(viewport))
        {
            child
                .as_widget()
                .draw(state, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Flow<'a>> for Element<'a, Message> {
    fn from(flow: Flow<'a>) -> Self {
        Element::new(flow)
    }
}

pub(super) fn divider(state: &SettingsWindow) -> Element<'_, Message> {
    let c = state.scheme();
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(move |_t| container::background(c.outline_variant))
        .into()
}

pub(super) fn note<'a>(
    state: &SettingsWindow,
    body: impl IntoFragment<'a>,
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
    label: impl IntoFragment<'a>,
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
    label: impl IntoFragment<'a>,
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
                c.surface_container_highest
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
    label: impl IntoFragment<'a>,
    lens: Lens<String>,
    placeholder: &str,
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
    label: impl IntoFragment<'a>,
    value: &str,
    placeholder: &str,
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
        .font(m3::field_font(value))
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
    label: impl IntoFragment<'a>,
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
                    Background::Color(c.secondary_container),
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
