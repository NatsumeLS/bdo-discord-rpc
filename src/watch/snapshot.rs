use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{stamp, summarize, Status, Watcher};
use crate::phase;
use crate::read::capture::{CaptureState, Captured};
use crate::read::game;
use crate::read::profile::{self, Profile, LIFE_SKILLS};
use crate::region;
use crate::show::presence::PLACEHOLDERS;
use crate::ui::lang::tr;
use crate::ui::tray::Health;
use rust_i18n::t;

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub health: Health,
    pub line: String,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub placeholders: BTreeMap<String, String>,
    #[serde(default)]
    pub server_key: Option<String>,
    #[serde(default)]
    pub character_ids: Vec<String>,
    #[serde(default)]
    pub detected: Detected,
}

// What each field that fills itself when blank would use right now.
#[derive(Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Detected {
    pub family: Option<String>,
    pub region: Option<String>,
    pub game_root: Option<String>,
    pub user_data_dir: Option<String>,
    pub profile_url: Option<String>,
    pub search_url: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub attention: bool,
}

impl Snapshot {
    pub fn read(path: &Path) -> Option<Snapshot> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

impl Watcher<'_> {
    /// The Overview's Capture row, flagged whenever the files stand in.
    fn capture_row(&self) -> (String, bool) {
        let kind = self.capture_kind();
        let text = match (kind, self.capture_state()) {
            ("failed", CaptureState::Failed(error)) => {
                t!("overview.capture_failed", error = error).into_owned()
            }
            _ => tr(&format!("overview.capture_{kind}")).to_string(),
        };
        (text, !matches!(kind, "starting" | "listening"))
    }

    /// Flagged when the opcodes were read from another build.
    fn client_row(&self) -> Option<(String, bool)> {
        let version = self.game.as_ref()?.version?;
        let keys = region::wire(Some(version)).map(|(build, _)| build);
        Some(match keys {
            Some(keys) if keys != version => (
                t!("overview.client_mismatch", version = version, keys = keys).into_owned(),
                true,
            ),
            _ => (version.to_string(), false),
        })
    }

    fn log_row(&self) -> Option<(String, bool)> {
        let tail = &self.game.as_ref()?.tail;
        let file = tail.file_name()?;
        Some(if tail.capped {
            (t!("overview.log_capped", file = file).into_owned(), true)
        } else {
            (file, false)
        })
    }

    fn phase_row(&self, captured: &Captured) -> String {
        let stable = &self.session.stable;
        let text = phase::display(stable.phase).to_string();
        if stable.phase.is_none() {
            return text;
        }
        let from_capture = captured
            .phase
            .is_some_and(|(_, since)| Some(since) == stable.phase_since);
        sourced(
            text,
            if from_capture {
                "overview.source_capture"
            } else {
                "overview.source_log"
            },
        )
    }

    fn server_row(&self, captured: &Captured) -> Option<(String, bool)> {
        let stable = &self.session.stable;
        let key = stable.game_server.as_deref()?;
        let (text, attention) = match self.config.servers.get(key) {
            Some(name) => (t!("overview.server_named", key = key, name = name), false),
            None => (t!("overview.server_unnamed", key = key), true),
        };
        let source = if captured.server == stable.game_server {
            "overview.source_capture"
        } else {
            "overview.source_log"
        };
        Some((sourced(text.into_owned(), source), attention))
    }

    fn family_row(&self, captured: &Captured) -> Option<(String, bool)> {
        let name = self.derived.family.clone()?;
        let (text, attention) = if self.families > 1 && !self.family_chosen() {
            let several = t!(
                "overview.family_several",
                name = name,
                count = self.families
            );
            (several.into_owned(), true)
        } else {
            (name, false)
        };
        let source = if self.family_chosen() {
            "overview.source_settings"
        } else if captured.family == self.derived.family {
            "overview.source_capture"
        } else {
            "overview.source_files"
        };
        Some((sourced(text, source), attention))
    }

    /// Only while the game runs, since that is when it connects.
    fn discord_row(&self) -> Option<(String, bool)> {
        self.game.as_ref()?;
        Some(match self.link.client {
            Some(_) => (tr("overview.discord_connected"), false),
            None => (
                t!("overview.discord_retrying", seconds = self.link.retry.held).into_owned(),
                true,
            ),
        })
    }

    /// What Discord shows now, as the log words it.
    fn presence_row(&self) -> Option<String> {
        self.link.client.as_ref()?;
        let hidden = self
            .session
            .stable
            .phase
            .is_some_and(|phase| !self.config.phase(phase).report);
        Some(match &self.derived.last_sent {
            Some(_) => summarize(&self.derived.last_sent),
            None if hidden => tr("overview.presence_not_broadcast"),
            None => tr("overview.presence_nothing"),
        })
    }

    /// Flagged when it is failing, or its owner hides the stats.
    fn profile_row(&self) -> Option<(String, bool)> {
        if !self.config.profile.enabled {
            return Some((tr("overview.profile_off"), false));
        }
        let fetched = self
            .derived
            .profile
            .as_ref()
            .and_then(|p| stamp(Some(p.fetched_at)));
        let seconds = self.profile_retry.held;
        Some(match (&self.profile_error, fetched) {
            (Some(error), Some(fetched)) => (
                t!(
                    "overview.profile_failing",
                    fetched = fetched,
                    seconds = seconds,
                    error = error
                )
                .into_owned(),
                true,
            ),
            (Some(error), None) => (
                t!("overview.profile_never", seconds = seconds, error = error).into_owned(),
                true,
            ),
            (None, Some(fetched)) if self.derived.profile.as_ref().is_some_and(|p| p.hidden) => (
                t!("overview.profile_hidden", fetched = fetched).into_owned(),
                true,
            ),
            (None, fetched) => (fetched?, false),
        })
    }

    fn character_row(&self, captured: &Captured) -> Option<(String, bool)> {
        let s = &self.session;
        let Some(key) = s.character.as_deref() else {
            let missed = self.config.identity.show_character
                && s.missed_for.is_some()
                && s.missed_for == s.state.phase_since;
            return missed.then(|| (tr("overview.character_unreadable"), true));
        };
        let (text, attention) = match self.character_name(key) {
            Some(name) if self.derived.unmatched.as_ref() == Some(&name) => (
                t!("overview.character_not_on_profile", key = key, name = name),
                true,
            ),
            Some(name) => {
                let who = self
                    .derived
                    .profile
                    .as_ref()
                    .and_then(|p| p.character(&name))
                    .map_or(name, describe);
                (t!("overview.character_named", key = key, name = who), false)
            }
            None => (t!("overview.character_unnamed", key = key), true),
        };
        let from_capture = captured.character.as_ref().is_some_and(|(id, _)| id == key);
        let source = if from_capture {
            "overview.source_capture"
        } else {
            "overview.source_files"
        };
        Some((sourced(text.into_owned(), source), attention))
    }

    /// The place as the presence shows it by default, node and territory.
    fn location(&self) -> Option<String> {
        let place = self.captured().place;
        let parts: Vec<&str> = [place.node.as_str(), place.territory.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
        (!parts.is_empty()).then(|| parts.join(" - "))
    }

    pub(super) fn snapshot(&self, status: &Status) -> Snapshot {
        // A flagged row is one the user has something to do about.
        let flagged = |label: String, value: Option<(String, bool)>| {
            value.map(|(value, attention)| Row {
                label,
                value,
                attention,
            })
        };
        let row = |label: String, value: Option<String>| flagged(label, value.map(|v| (v, false)));
        let group = |title: String, rows: Vec<Option<Row>>| Group {
            title,
            rows: rows.into_iter().flatten().collect(),
        };
        let game = self.game.as_ref();
        let stable = &self.session.stable;
        let profile = self.derived.profile.as_ref();
        let captured = self.captured_raw();
        Snapshot {
            health: status.health,
            line: status.line.clone(),
            service: self.service().map(str::to_string),
            server_key: stable.game_server.clone(),
            character_ids: self.derived.user_cache.character_ids.clone(),
            detected: {
                let service = self.service();
                Detected {
                    family: self.derived.user_cache.newest_family.clone(),
                    region: service.map(region::name),
                    game_root: game.map(|g| g.root.display().to_string()),
                    user_data_dir: game::default_user_data_dir()
                        .map(|dir| dir.display().to_string()),
                    profile_url: self
                        .derived
                        .profile_url
                        .clone()
                        .or_else(|| profile.and_then(|p| p.url.clone())),
                    search_url: service.and_then(region::get).map(|r| r.search.clone()),
                }
            },
            placeholders: {
                let ctx = self.presence_context();
                PLACEHOLDERS
                    .iter()
                    .map(|p| (p.name, (p.value)(&ctx)))
                    .filter(|(_, value)| !value.is_empty())
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect()
            },
            groups: vec![
                group(
                    tr("overview.game"),
                    vec![
                        row(
                            tr("overview.folder"),
                            game.map(|g| g.root.display().to_string()),
                        ),
                        row(tr("overview.region"), game.and_then(|_| self.region())),
                        flagged(tr("overview.client"), self.client_row()),
                        flagged(tr("overview.log_file"), self.log_row()),
                        flagged(tr("overview.capture"), Some(self.capture_row())),
                    ],
                ),
                group(
                    tr("overview.session"),
                    vec![
                        flagged(tr("overview.discord"), self.discord_row()),
                        row(tr("overview.presence"), self.presence_row()),
                        row(tr("overview.phase"), Some(self.phase_row(&captured))),
                        row(tr("overview.phase_since"), stamp(stable.phase_since)),
                        row(tr("overview.session_start"), stamp(stable.session_start)),
                        flagged(tr("overview.server"), self.server_row(&captured)),
                        row(tr("overview.location"), self.location()),
                    ],
                ),
                group(
                    tr("overview.you"),
                    vec![
                        flagged(tr("overview.family"), self.family_row(&captured)),
                        flagged(tr("overview.character"), self.character_row(&captured)),
                        row(
                            tr("overview.main"),
                            profile.and_then(Profile::main).map(describe),
                        ),
                        row(
                            tr("overview.family_created"),
                            profile.and_then(Profile::created_local),
                        ),
                        row(tr("overview.guild"), profile.and_then(|p| p.guild.clone())),
                        row(
                            tr("overview.gear_score"),
                            profile.and_then(|p| p.gear_score.clone()),
                        ),
                        row(
                            tr("overview.energy"),
                            profile.and_then(|p| p.energy.clone()),
                        ),
                        row(
                            tr("overview.contribution"),
                            profile.and_then(|p| p.contribution.clone()),
                        ),
                        flagged(tr("overview.profile_fetched"), self.profile_row()),
                    ],
                ),
                group(
                    tr("overview.life_skills"),
                    LIFE_SKILLS
                        .iter()
                        .map(|skill| {
                            row(
                                tr(&format!("life.{}", skill.to_lowercase())),
                                profile.and_then(|p| p.life_skills.get(*skill).cloned()),
                            )
                        })
                        .collect(),
                ),
            ],
        }
    }
}

// Where a reading came from, so a fall back shows on the page.
fn sourced(value: String, source: &str) -> String {
    t!("overview.sourced", value = value, source = tr(source)).into_owned()
}

// "<name>, <class>, Lv. <level>", or without the level when the page has none.
fn describe(character: &profile::Character) -> String {
    let class = t!(
        "overview.class",
        name = character.name,
        class = character.class
    );
    match character.level {
        Some(level) => t!("overview.class_level", who = class, level = level).into_owned(),
        None => class.into_owned(),
    }
}
