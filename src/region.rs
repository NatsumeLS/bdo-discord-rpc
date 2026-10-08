use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Deserialize;

use crate::{data, win};

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
    static REGIONS: Mutex<Option<(u32, &'static BTreeMap<String, Region>)>> = Mutex::new(None);
    data::cached(&REGIONS, || {
        toml::from_str(&data::load("service/regions.toml")).unwrap_or_else(|e| {
            win::warn(&format!(
                "Data: regions.toml does not parse, so no Region is known ({e})"
            ));
            BTreeMap::new()
        })
    })
}

fn wires() -> &'static BTreeMap<u32, Wire> {
    static WIRES: Mutex<Option<(u32, &'static BTreeMap<u32, Wire>)>> = Mutex::new(None);
    data::cached(&WIRES, || {
        let table = match data::load("service/opcodes.toml").parse::<toml::Table>() {
            Ok(table) => table,
            Err(e) => {
                win::warn(&format!(
                    "Data: opcodes.toml does not parse, so the Capture has no Opcodes ({e})"
                ));
                return BTreeMap::new();
            }
        };
        // Each build on its own, so one with a typo loses only itself.
        table
            .into_iter()
            .filter_map(|(build, keys)| {
                let wire = build
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())
                    .and_then(|b| keys.try_into().map(|w| (b, w)).map_err(|e| e.to_string()));
                wire.inspect_err(|e| {
                    win::warn(&format!(
                        "Data: [{build}] in opcodes.toml does not parse, skipping it ({e})"
                    ));
                })
                .ok()
            })
            .collect()
    })
}

/// The opcodes for this client build, or else the newest, with the
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
