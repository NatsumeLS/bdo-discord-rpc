use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Region {
    pub name: String,
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
    // Flattened, so a region missing any capture key has no capture rather
    // than failing the whole file.
    #[serde(flatten)]
    pub capture: Option<Wire>,
}

/// What the packet capture decodes, which moves with each client build.
/// Offsets count from the start of the frame, header included.
#[derive(Deserialize)]
pub struct Wire {
    /// The client build these were read from.
    pub version: u32,
    pub world_port: u16,
    pub list: u16,
    pub enter: u16,
    pub enter_length: usize,
    pub enter_id: usize,
    pub enter_name: usize,
    pub enter_family: usize,
    pub enter_position: usize,
    pub position: u16,
    pub position_at: usize,
    pub server_hosts: String,
    pub server_count: u32,
}

impl Wire {
    /// The host of world server `n`, like `game07` for `game{nn}`.
    pub fn server_host(&self, n: u32) -> String {
        self.server_hosts.replace("{nn}", &format!("{n:02}"))
    }
}

fn all() -> &'static BTreeMap<String, Region> {
    static REGIONS: OnceLock<BTreeMap<String, Region>> = OnceLock::new();
    REGIONS
        .get_or_init(|| toml::from_str(include_str!("../assets/regions.toml")).unwrap_or_default())
}

pub fn get(code: &str) -> Option<&'static Region> {
    all()
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(code.trim()))
        .map(|(_, region)| region)
}

// An unknown code is shown as itself, so a new service still reads sensibly.
pub fn name(code: &str) -> String {
    get(code).map_or_else(|| code.to_string(), |region| region.name.clone())
}

// Without a known region, every region's names, each once.
pub fn servers(code: Option<&str>) -> Vec<&'static String> {
    match code.and_then(get) {
        Some(region) => region.servers.iter().collect(),
        None => {
            let mut seen = Vec::new();
            for name in all().values().flat_map(|region| &region.servers) {
                if !seen.contains(&name) {
                    seen.push(name);
                }
            }
            seen
        }
    }
}
