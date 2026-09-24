#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Home,
    Login,
    ServerSelect,
    CharacterCreate,
    Lobby,
    Loading,
    Play,
}

impl Phase {
    pub const ALL: [Phase; 7] = [
        Phase::Home,
        Phase::Login,
        Phase::ServerSelect,
        Phase::CharacterCreate,
        Phase::Lobby,
        Phase::Loading,
        Phase::Play,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Phase::Home => "home",
            Phase::Login => "login",
            Phase::ServerSelect => "server_select",
            Phase::CharacterCreate => "character_create",
            Phase::Lobby => "lobby",
            Phase::Loading => "loading",
            Phase::Play => "play",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Home => "Starting up",
            Phase::Login => "Main Menu",
            Phase::ServerSelect => "Server Select",
            Phase::CharacterCreate => "Character Creation",
            Phase::Lobby => "Character Select",
            Phase::Loading => "Loading",
            Phase::Play => "In Game",
        }
    }

    pub fn from_processor(name: &str) -> Option<Phase> {
        Some(match name {
            "Home" => Phase::Home,
            "Login" => Phase::Login,
            "ServerSelect" => Phase::ServerSelect,
            "CaptureFace" => Phase::CharacterCreate,
            "Lobby" => Phase::Lobby,
            "Loading" => Phase::Loading,
            "Play" => Phase::Play,
            _ => return None,
        })
    }
}

pub fn label(phase: Option<Phase>) -> &'static str {
    match phase {
        Some(phase) => phase.label(),
        None => "Unknown",
    }
}

// `label` stays English for the log. This is the one the windows and tray show.
pub fn display(phase: Option<Phase>) -> &'static str {
    let key = phase.map_or("unknown", Phase::key);
    crate::ui::lang::tr(&format!("phase.{key}"))
}
