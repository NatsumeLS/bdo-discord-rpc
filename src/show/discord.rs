use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::show::presence::PresenceFields;

pub struct Presence {
    client: DiscordIpcClient,
}

impl Presence {
    pub fn connect(client_id: &str) -> Result<Presence, String> {
        if client_id.trim().is_empty() {
            return Err("No client_id set in the Config".into());
        }

        let mut client = DiscordIpcClient::new(client_id);
        client
            .connect()
            .map_err(|e| format!("Could not connect, is Discord running? ({e})"))?;

        Ok(Presence { client })
    }

    pub fn set(&mut self, fields: &PresenceFields) -> Result<(), String> {
        self.client
            .set_activity(to_activity(fields))
            .map_err(|e| format!("Could not set the Presence: {e}"))
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.client
            .clear_activity()
            .map_err(|e| format!("Could not clear the Presence: {e}"))
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let _ = self.client.clear_activity();
        let _ = self.client.close();
    }
}

pub fn preview(fields: &PresenceFields) -> String {
    serde_json::to_string_pretty(&to_activity(fields))
        .unwrap_or_else(|e| format!("Could not serialize the Activity: {e}"))
}

fn to_activity(fields: &PresenceFields) -> activity::Activity<'_> {
    let mut act = activity::Activity::new();

    if let Some(details) = &fields.details {
        act = act.details(details);
    }
    if let Some(state) = &fields.state {
        act = act.state(state);
    }
    if let Some(start) = fields.start {
        act = act.timestamps(activity::Timestamps::new().start(start));
    }
    if !fields.buttons.is_empty() {
        act = act.buttons(
            fields
                .buttons
                .iter()
                .map(|(label, url)| activity::Button::new(label, url))
                .collect(),
        );
    }

    let assets_wanted = [
        &fields.large_image,
        &fields.large_text,
        &fields.small_image,
        &fields.small_text,
    ];
    if assets_wanted.iter().any(|value| value.is_some()) {
        let mut assets = activity::Assets::new();
        if let Some(value) = &fields.large_image {
            assets = assets.large_image(value);
        }
        if let Some(value) = &fields.large_text {
            assets = assets.large_text(value);
        }
        if let Some(value) = &fields.small_image {
            assets = assets.small_image(value);
        }
        if let Some(value) = &fields.small_text {
            assets = assets.small_text(value);
        }
        act = act.assets(assets);
    }

    act
}
