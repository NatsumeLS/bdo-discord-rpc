use crate::config::Config;
use crate::read::log_tail::GameState;
use crate::read::profile::Profile;

pub struct Context {
    pub family: String,
    pub region: String,
    pub server: String,
    pub phase: String,
    pub character: String,
    pub class: String,
    pub level: String,
    pub class_image: String,

    pub main_character: String,
    pub main_class: String,
    pub main_level: String,
    pub main_class_image: String,
    pub guild: String,
    pub gear_score: String,
    pub energy: String,
    pub contribution: String,
    pub profile_url: String,
}

pub fn context(
    config: &Config,
    state: &GameState,
    family: Option<&str>,
    region: &str,
    character: Option<&str>,
    profile: Option<&Profile>,
) -> Context {
    let main = profile.and_then(Profile::main);
    let text = |value: Option<&String>| value.cloned().unwrap_or_default();

    // A detected character never borrows the main's details, it reads as Unknown.
    let detected = config
        .identity
        .show_character
        .then_some(character)
        .flatten();
    let named = detected.and_then(|key| config.characters.get(key));
    let current = match detected {
        Some(_) => named.and_then(|name| profile.and_then(|p| p.character(name))),
        None => main,
    };
    let known = |value: Option<String>| match detected {
        Some(_) => value.unwrap_or_else(|| config.display.unknown.clone()),
        None => value.unwrap_or_default(),
    };

    Context {
        family: if config.identity.show_family {
            family
                .map(str::to_string)
                .or_else(|| profile.and_then(|p| p.family.clone()))
                .unwrap_or_default()
        } else {
            String::new()
        },
        region: if config.display.show_region {
            region.to_string()
        } else {
            String::new()
        },
        server: match (config.display.show_server, state.game_server.as_deref()) {
            (true, Some(host)) => config.server_name(host),
            _ => String::new(),
        },
        phase: state.phase.map(|p| p.key().to_string()).unwrap_or_default(),
        character: match detected {
            Some(_) => known(named.cloned()),
            None if config.identity.show_character => {
                main.map(|c| c.name.clone()).unwrap_or_default()
            }
            None => String::new(),
        },
        class: known(current.map(|c| c.class.clone()).filter(|c| !c.is_empty())),
        level: known(current.and_then(|c| c.level).map(|l| l.to_string())),
        class_image: current
            .and_then(|c| c.class_image.clone())
            .unwrap_or_default(),

        main_character: if config.identity.show_character {
            main.map(|c| c.name.clone()).unwrap_or_default()
        } else {
            String::new()
        },
        main_class: main.map(|c| c.class.clone()).unwrap_or_default(),
        main_class_image: main.and_then(|c| c.class_image.clone()).unwrap_or_default(),
        main_level: main
            .and_then(|c| c.level)
            .map(|l| l.to_string())
            .unwrap_or_default(),
        guild: text(profile.and_then(|p| p.guild.as_ref())),
        gear_score: text(profile.and_then(|p| p.gear_score.as_ref())),
        energy: text(profile.and_then(|p| p.energy.as_ref())),
        contribution: text(profile.and_then(|p| p.contribution.as_ref())),
        profile_url: text(profile.and_then(|p| p.url.as_ref())),
    }
}

#[derive(PartialEq, Eq)]
pub struct PresenceFields {
    pub details: Option<String>,
    pub state: Option<String>,
    pub start: Option<i64>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    pub buttons: Vec<(String, String)>,
}

const GAME_NAME: &str = "Black Desert";
pub const BUTTON_LABEL_MAX: usize = 32;
pub const BUTTON_URL_MAX: usize = 512;

pub fn is_button_url(url: &str) -> bool {
    url.len() <= BUTTON_URL_MAX && (url.starts_with("https://") || url.starts_with("http://"))
}

// Discord rejects the whole activity over one bad button, so it is dropped.
fn button(label: &str, url: &str, ctx: &Context) -> Option<(String, String)> {
    let label = expand(label, ctx);
    let url = expand(url, ctx);
    let fits = !label.is_empty() && label.chars().count() <= BUTTON_LABEL_MAX;
    (fits && is_button_url(&url)).then_some((label, url))
}

pub type Placeholder = (&'static str, fn(&Context) -> &str);

pub const PLACEHOLDERS: &[Placeholder] = &[
    ("family", |c| &c.family),
    ("region", |c| &c.region),
    ("server", |c| &c.server),
    ("phase", |c| &c.phase),
    ("character", |c| &c.character),
    ("class", |c| &c.class),
    ("level", |c| &c.level),
    ("class_image", |c| &c.class_image),
    ("main_character", |c| &c.main_character),
    ("main_class", |c| &c.main_class),
    ("main_class_image", |c| &c.main_class_image),
    ("main_level", |c| &c.main_level),
    ("guild", |c| &c.guild),
    ("gear_score", |c| &c.gear_score),
    ("energy", |c| &c.energy),
    ("contribution", |c| &c.contribution),
    ("profile_url", |c| &c.profile_url),
];

pub fn is_placeholder(name: &str) -> bool {
    PLACEHOLDERS.iter().any(|&(known, _)| known == name)
}

pub fn expand(template: &str, ctx: &Context) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];

        let Some(close) = after.find('}') else {
            out.push_str(after);
            rest = "";
            break;
        };

        let name = &after[1..close];
        match PLACEHOLDERS.iter().find(|&&(known, _)| known == name) {
            Some((_, value)) => out.push_str(value(ctx)),
            None => out.push_str(&after[..=close]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);

    tidy(&out)
}

fn tidy(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let is_sep = |c: char| matches!(c, '-' | ':' | '|' | ',');
    let chars: Vec<char> = collapsed.chars().collect();

    let mut out = String::with_capacity(collapsed.len());
    for (i, &c) in chars.iter().enumerate() {
        if is_sep(c) {
            if out.trim().is_empty() {
                continue;
            }
            if chars[i + 1..].iter().all(|&n| n == ' ' || is_sep(n)) {
                break;
            }
        }
        out.push(c);
    }

    out.trim().to_string()
}

fn usable(value: String) -> Option<String> {
    (value.chars().count() >= 2).then_some(value)
}

pub fn build(config: &Config, state: &GameState, ctx: &Context) -> Option<PresenceFields> {
    let phase = state.phase?;
    let phase_config = config.phase(phase);
    if !phase_config.report {
        return None;
    }

    let start = match config.display.timer_mode.as_str() {
        "session" => state.session_start,
        "phase" => state.phase_since,
        _ => None,
    };

    let mut large_image = usable(expand(&phase_config.large_image, ctx));
    let mut large_text = usable(expand(&phase_config.large_text, ctx));
    let mut small_image = usable(expand(&phase_config.small_image, ctx));

    let game_icon = usable(expand(&config.display.game_icon, ctx));
    if large_image.is_none() {
        // A caption that described an image we did not get goes with it.
        if !phase_config.large_image.trim().is_empty() {
            large_text = None;
        }
        large_image = game_icon;
        large_text = large_text.or_else(|| Some(GAME_NAME.to_string()));
    } else {
        small_image = small_image.or(game_icon);
    }

    Some(PresenceFields {
        details: usable(expand(&phase_config.details, ctx)),
        state: usable(expand(&phase_config.state, ctx)),
        start,
        large_image,
        large_text,
        small_image,
        small_text: usable(expand(&phase_config.small_text, ctx)),
        buttons: [
            (&phase_config.button_label, &phase_config.button_url),
            (
                &phase_config.second_button_label,
                &phase_config.second_button_url,
            ),
        ]
        .into_iter()
        .filter_map(|(label, url)| button(label, url, ctx))
        .collect(),
    })
}
