use std::collections::BTreeMap;
use std::sync::Mutex;

use iced::widget::{button, container, scrollable, stack, text, text_input, Space};
use iced::{border, Background, Border, Color, Element, Length, Shadow, Vector};
use material_color_rs::dynamiccolor::MaterialDynamicColors as Roles;
use material_color_rs::{DynamicScheme, Hct, TonalPalette, Variant};

use crate::config::Config;

// Not a cache: see `Scheme::new`.
static RESOLVED: Mutex<BTreeMap<(u32, bool, Palette), Scheme>> = Mutex::new(BTreeMap::new());

pub const FALLBACK_SEED: u32 = 0xF0_80_80;

const ERROR_HUE: f64 = 25.0;
const ERROR_CHROMA: f64 = 120.0;
const ERROR_TONE: (f64, f64) = (60.0, 40.0);

const WARNING_HUE: f64 = 75.0;
const WARNING_CHROMA: f64 = 90.0;
const WARNING_TONE: (f64, f64) = (70.0, 40.0);

pub fn parse_hex(text: &str) -> Option<u32> {
    let body = text.trim().trim_start_matches('#');
    (body.len() == 6 && body.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(body, 16).ok())
        .flatten()
}

pub fn seed(config: &Config) -> u32 {
    parse_hex(&config.theme.accent).unwrap_or(FALLBACK_SEED)
}

pub fn dark(config: &Config) -> bool {
    !config.theme.mode.eq_ignore_ascii_case("light")
}

pub fn theme(config: &Config) -> iced::Theme {
    if dark(config) {
        iced::Theme::Dark
    } else {
        iced::Theme::Light
    }
}

pub fn palette(config: &Config) -> Palette {
    Palette::parse(&config.theme.palette).unwrap_or(Palette::TonalSpot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Palette {
    TonalSpot,
    Neutral,
    Monochrome,
    Vibrant,
    Expressive,
    Rainbow,
    FruitSalad,
}

impl Palette {
    pub const ALL: [Palette; 7] = [
        Palette::TonalSpot,
        Palette::Neutral,
        Palette::Monochrome,
        Palette::Vibrant,
        Palette::Expressive,
        Palette::Rainbow,
        Palette::FruitSalad,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Palette::TonalSpot => "tonal_spot",
            Palette::Neutral => "neutral",
            Palette::Monochrome => "monochrome",
            Palette::Vibrant => "vibrant",
            Palette::Expressive => "expressive",
            Palette::Rainbow => "rainbow",
            Palette::FruitSalad => "fruit_salad",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Palette::TonalSpot => "Tonal Spot",
            Palette::Neutral => "Neutral",
            Palette::Monochrome => "Monochrome",
            Palette::Vibrant => "Vibrant",
            Palette::Expressive => "Expressive",
            Palette::Rainbow => "Rainbow",
            Palette::FruitSalad => "Fruit Salad",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let key = text.trim();
        Palette::ALL
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(key))
    }

    fn scheme(self, source: Hct, dark: bool) -> DynamicScheme {
        let hue = source.hue();
        let variant = self.variant();
        // (primary, secondary, tertiary, neutral, neutral_variant)
        let spread: [(f64, f64); 5] = match self {
            Palette::TonalSpot => [
                (hue, 36.0),
                (hue, 16.0),
                (hue + 60.0, 24.0),
                (hue, 6.0),
                (hue, 8.0),
            ],
            Palette::Neutral => [(hue, 12.0), (hue, 8.0), (hue, 16.0), (hue, 2.0), (hue, 2.0)],
            Palette::Monochrome => [(hue, 0.0), (hue, 0.0), (hue, 0.0), (hue, 0.0), (hue, 0.0)],
            Palette::Vibrant => {
                const HUES: [f64; 9] = [0.0, 41.0, 61.0, 101.0, 131.0, 181.0, 251.0, 301.0, 360.0];
                const SECOND: [f64; 8] = [18.0, 15.0, 10.0, 12.0, 15.0, 18.0, 15.0, 12.0];
                const THIRD: [f64; 8] = [35.0, 30.0, 20.0, 25.0, 30.0, 35.0, 30.0, 25.0];
                [
                    (hue, 200.0),
                    (rotated(hue, &HUES, &SECOND), 24.0),
                    (rotated(hue, &HUES, &THIRD), 32.0),
                    (hue, 10.0),
                    (hue, 12.0),
                ]
            }
            Palette::Expressive => {
                const HUES: [f64; 9] = [0.0, 21.0, 51.0, 121.0, 151.0, 191.0, 271.0, 321.0, 360.0];
                const SECOND: [f64; 8] = [45.0, 95.0, 45.0, 20.0, 45.0, 90.0, 45.0, 45.0];
                const THIRD: [f64; 8] = [120.0, 120.0, 20.0, 45.0, 20.0, 15.0, 20.0, 120.0];
                [
                    (hue + 240.0, 40.0),
                    (rotated(hue, &HUES, &SECOND), 24.0),
                    (rotated(hue, &HUES, &THIRD), 32.0),
                    (hue + 15.0, 8.0),
                    (hue + 15.0, 12.0),
                ]
            }
            Palette::Rainbow => [
                (hue, 48.0),
                (hue, 16.0),
                (hue + 60.0, 24.0),
                (hue, 0.0),
                (hue, 0.0),
            ],
            Palette::FruitSalad => [
                (hue - 50.0, 48.0),
                (hue - 50.0, 36.0),
                (hue, 36.0),
                (hue, 10.0),
                (hue, 16.0),
            ],
        };
        let tone = |(hue, chroma): (f64, f64)| TonalPalette::of(hue.rem_euclid(360.0), chroma);

        DynamicScheme::new(
            source,
            variant,
            dark,
            0.0,
            tone(spread[0]),
            tone(spread[1]),
            tone(spread[2]),
            tone(spread[3]),
            tone(spread[4]),
            Some(TonalPalette::of(ERROR_HUE, ERROR_CHROMA)),
        )
    }

    fn variant(self) -> Variant {
        match self {
            Palette::TonalSpot => Variant::TonalSpot,
            Palette::Neutral => Variant::Neutral,
            Palette::Monochrome => Variant::Monochrome,
            Palette::Vibrant => Variant::Vibrant,
            Palette::Expressive => Variant::Expressive,
            Palette::Rainbow => Variant::Rainbow,
            Palette::FruitSalad => Variant::FruitSalad,
        }
    }
}

fn rotated(hue: f64, breakpoints: &[f64; 9], rotations: &[f64; 8]) -> f64 {
    for index in 0..8 {
        if breakpoints[index] < hue && hue < breakpoints[index + 1] {
            return hue + rotations[index];
        }
    }
    hue
}

#[derive(Clone, Copy)]
pub struct Scheme {
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub surface: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub outline: Color,
    pub outline_variant: Color,
    pub error: Color,
    pub warning: Color,
}

impl Scheme {
    pub fn new(seed: u32, dark: bool, palette: Palette) -> Self {
        if let Some(hit) = RESOLVED.lock().unwrap().get(&(seed, dark, palette)) {
            return *hit;
        }

        // Leaked on purpose: material-color-rs caches roles by the scheme's
        // address, so a freed one hands its colors to the next. Without the
        // alpha byte a seed resolves to black.
        let scheme: &'static DynamicScheme = Box::leak(Box::new(
            palette.scheme(Hct::from_int(0xFF00_0000 | seed), dark),
        ));
        let role = |color: &material_color_rs::dynamiccolor::DynamicColor| {
            argb_to_color(color.get_argb(scheme))
        };

        let resolved = Scheme {
            primary: role(Roles::primary()),
            on_primary: role(Roles::on_primary()),
            primary_container: role(Roles::primary_container()),
            secondary_container: role(Roles::secondary_container()),
            on_secondary_container: role(Roles::on_secondary_container()),
            surface: role(Roles::surface()),
            on_surface: role(Roles::on_surface()),
            on_surface_variant: role(Roles::on_surface_variant()),
            surface_container: role(Roles::surface_container()),
            surface_container_high: role(Roles::surface_container_high()),
            outline: role(Roles::outline()),
            outline_variant: role(Roles::outline_variant()),
            // A tone of our own: M3's dark `error` is a pale salmon.
            error: argb_to_color(scheme.error_palette.get(if dark {
                ERROR_TONE.0
            } else {
                ERROR_TONE.1
            } as i32)),
            warning: argb_to_color(TonalPalette::of(WARNING_HUE, WARNING_CHROMA).get(if dark {
                WARNING_TONE.0
            } else {
                WARNING_TONE.1
            }
                as i32)),
        };

        RESOLVED
            .lock()
            .unwrap()
            .insert((seed, dark, palette), resolved);
        resolved
    }

    pub fn of(config: &Config) -> Self {
        Scheme::new(seed(config), dark(config), palette(config))
    }
}

pub fn describe(config: &Config) {
    let seed = seed(config);
    println!(
        "Accent    : #{seed:06X}{}",
        if parse_hex(&config.theme.accent).is_some() {
            ""
        } else {
            " (the configured Accent did not parse, using the Default)"
        }
    );
    let hex = |color: Color| {
        let rgba = color.into_rgba8();
        format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
    };
    let scheme = Scheme::of(config);
    println!(
        "Theme     : {} {} (primary {}, surface {}, error {})",
        if dark(config) { "Dark" } else { "Light" },
        palette(config).label(),
        hex(scheme.primary),
        hex(scheme.surface),
        hex(scheme.error)
    );
}

fn argb_to_color(argb: u32) -> Color {
    Color::from_rgb8(
        ((argb >> 16) & 0xFF) as u8,
        ((argb >> 8) & 0xFF) as u8,
        (argb & 0xFF) as u8,
    )
}

pub fn hsv_of(rgb: u32) -> (f32, f32, f32) {
    let [r, g, b] = [16, 8, 0].map(|shift| ((rgb >> shift) & 0xFF) as f32 / 255.0);
    let max = r.max(g).max(b);
    let span = max - r.min(g).min(b);

    let hue = if span == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / span) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / span + 2.0)
    } else {
        60.0 * ((r - g) / span + 4.0)
    };

    (
        hue.rem_euclid(360.0),
        if max == 0.0 { 0.0 } else { span / max },
        max,
    )
}

pub fn hsv_hex(hue: f32, saturation: f32, value: f32) -> String {
    let rgb = hsv_color(hue, saturation, value).into_rgba8();
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn hsv_color(hue: f32, saturation: f32, value: f32) -> Color {
    let chroma = value * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u8 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = value - chroma;
    Color::from_rgb(r + base, g + base, b + base)
}

pub fn layer(status: button::Status) -> f32 {
    match status {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.12,
        _ => 0.0,
    }
}

pub fn mix(base: Color, over: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color {
        r: base.r + (over.r - base.r) * t,
        g: base.g + (over.g - base.g) * t,
        b: base.b + (over.b - base.b) * t,
        a: base.a + (over.a - base.a) * t,
    }
}

pub fn field_style(c: Scheme, status: text_input::Status, wrong: bool) -> text_input::Style {
    let (outline, width) = match status {
        _ if wrong => (c.error, 2.0),
        text_input::Status::Focused { .. } => (c.primary, 2.0),
        text_input::Status::Hovered => (c.on_surface, 1.0),
        _ => (c.outline, 1.0),
    };
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: outline,
            width,
            radius: shape::EXTRA_SMALL.into(),
        },
        icon: c.on_surface_variant,
        placeholder: c.outline,
        value: if wrong { c.error } else { c.on_surface },
        selection: c.primary_container,
    }
}

pub fn pill(background: Color, text_color: Color) -> button::Style {
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: border::rounded(shape::FULL),
        shadow: Shadow::default(),
        // The default leaves it off without iced's `crisp` feature.
        snap: true,
    }
}

pub fn filled<'a, M: Clone + 'a>(c: Scheme, label: &'a str, message: Option<M>) -> Element<'a, M> {
    button(text(label).size(type_scale::LABEL_LARGE))
        .padding([10, 24])
        .on_press_maybe(message)
        .style(move |_theme, status| {
            if matches!(status, button::Status::Disabled) {
                return pill(mix(c.on_surface, c.surface, 0.88), c.on_surface_variant);
            }
            button::Style {
                shadow: Shadow {
                    color: Color {
                        a: 0.28,
                        ..Color::BLACK
                    },
                    offset: Vector::new(0.0, 1.0),
                    blur_radius: 3.0,
                },
                ..pill(mix(c.primary, c.on_primary, layer(status)), c.on_primary)
            }
        })
        .into()
}

pub fn plain<'a, M: Clone + 'a>(c: Scheme, label: &'a str, message: Option<M>) -> Element<'a, M> {
    button(text(label).size(type_scale::LABEL_LARGE))
        .padding([10, 16])
        .on_press_maybe(message)
        .style(move |_theme, status| {
            pill(
                Color {
                    a: layer(status),
                    ..c.primary
                },
                if matches!(status, button::Status::Disabled) {
                    c.on_surface_variant
                } else {
                    c.primary
                },
            )
        })
        .into()
}

pub fn chip<'a, M: Clone + 'a>(
    c: Scheme,
    label: &'a str,
    selected: bool,
    message: M,
) -> Element<'a, M> {
    button(text(label).size(type_scale::LABEL_LARGE))
        .padding([8, 16])
        .on_press(message)
        .style(move |_theme, status| {
            let (base, fg, outline) = if selected {
                (c.secondary_container, c.on_secondary_container, 0.0)
            } else {
                (Color::TRANSPARENT, c.on_surface_variant, 1.0)
            };
            button::Style {
                border: border::rounded(shape::SMALL)
                    .color(c.outline)
                    .width(outline),
                ..pill(mix(base, c.on_surface, layer(status)), fg)
            }
        })
        .into()
}

// Room the clear button takes at a field's right end, for the field's padding.
pub const CLEAR_ROOM: f32 = 40.0;

// An × over the field's right end, only while there is something to clear.
pub fn clearable<'a, M: Clone + 'a>(
    c: Scheme,
    field: impl Into<Element<'a, M>>,
    clear: Option<M>,
) -> Element<'a, M> {
    // Always a stack, or the first letter typed swaps the widget and drops its focus.
    let overlay: Element<'a, M> = match clear {
        Some(clear) => button(text("\u{00D7}").size(type_scale::TITLE_MEDIUM))
            .padding([2, 10])
            .on_press(clear)
            .style(move |_theme, status| {
                pill(
                    Color {
                        a: layer(status),
                        ..c.on_surface
                    },
                    c.on_surface_variant,
                )
            })
            .into(),
        None => Space::new().into(),
    };
    stack![
        field.into(),
        container(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::End)
            .align_y(iced::Alignment::Center)
            .padding([0, 6]),
    ]
    .into()
}

pub fn scroll_style(c: Scheme, status: scrollable::Status) -> scrollable::Style {
    let hovered = match status {
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => is_vertical_scrollbar_hovered || is_horizontal_scrollbar_hovered,
        scrollable::Status::Dragged { .. } => true,
        _ => false,
    };

    let rail = scrollable::Rail {
        background: Some(Background::Color(c.surface_container)),
        border: border::rounded(shape::FULL),
        scroller: scrollable::Scroller {
            background: Background::Color(if hovered {
                c.on_surface_variant
            } else {
                c.outline
            }),
            border: border::rounded(shape::FULL),
        },
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(c.surface_container_high),
            border: border::rounded(shape::FULL)
                .color(c.outline_variant)
                .width(1.0),
            shadow: Shadow::default(),
            icon: c.on_surface_variant,
        },
    }
}

pub mod shape {
    pub const EXTRA_SMALL: f32 = 4.0;
    pub const SMALL: f32 = 8.0;
    pub const MEDIUM: f32 = 12.0;
    pub const FULL: f32 = 999.0;
}

pub mod type_scale {
    pub const HEADLINE_SMALL: f32 = 24.0;
    pub const TITLE_MEDIUM: f32 = 16.0;
    pub const BODY_LARGE: f32 = 16.0;
    pub const BODY_MEDIUM: f32 = 14.0;
    pub const LABEL_LARGE: f32 = 14.0;
}
