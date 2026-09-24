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
