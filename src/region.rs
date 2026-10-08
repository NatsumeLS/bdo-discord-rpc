use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Region {
    pub name: String,
    pub search: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

/// What the packet capture decodes, which moves with each client build.
/// Offsets count from the start of the frame, header included.
#[derive(Deserialize)]
pub struct Wire {
    pub list: u16,
    pub enter: u16,
    pub enter_length: usize,
    pub enter_id: usize,
    pub enter_name: usize,
    pub enter_family: usize,
    pub enter_position: usize,
    pub position: u16,
    pub position_at: usize,
}

fn all() -> &'static BTreeMap<String, Region> {
    static REGIONS: OnceLock<BTreeMap<String, Region>> = OnceLock::new();
    REGIONS
        .get_or_init(|| toml::from_str(include_str!("../assets/regions.toml")).unwrap_or_default())
}

fn wires() -> &'static BTreeMap<u32, Wire> {
    static WIRES: OnceLock<BTreeMap<u32, Wire>> = OnceLock::new();
    WIRES.get_or_init(|| {
        // Each build on its own, so one with a typo loses only itself.
        include_str!("../assets/opcodes.toml")
            .parse::<toml::Table>()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(build, keys)| Some((build.parse().ok()?, keys.try_into().ok()?)))
            .collect()
    })
}

/// The capture keys for this client build, or else the newest, with the
/// build they were read from.
pub fn wire(build: Option<u32>) -> Option<(u32, &'static Wire)> {
    let wires = wires();
    build
        .and_then(|b| wires.get_key_value(&b))
        .or_else(|| wires.last_key_value())
        .map(|(&b, wire)| (b, wire))
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
