use std::collections::HashMap;
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::{data, win};

// The game's own tables, as they are dumped to assets/client.
#[derive(Deserialize)]
pub struct Exploration {
    key: u32,
    enabled: bool,
    main: bool,
    radius: f32,
    anchor: [f32; 3],
}

#[derive(Deserialize)]
pub struct Waypoints {
    points: Vec<Waypoint>,
    links: Vec<(u32, u32)>,
}

#[derive(Deserialize)]
struct Waypoint {
    key: u32,
    position: [f32; 3],
}

#[derive(Deserialize)]
pub struct Region {
    territory: u8,
    position: [f32; 3],
}

/// Table, then id, then its text: 12 territory names, 29 node names.
pub type Localization = HashMap<String, HashMap<String, String>>;

struct Node {
    place: Place,
    x: f32,
    z: f32,
    radius: f32,
}

/// Where the character is, by node and the territory that node is in.
#[derive(Clone, Default, PartialEq)]
pub struct Place {
    pub node: String,
    pub territory: String,
}

fn parse<T: DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_str(&data::load(name))
        .inspect_err(|e| {
            win::warn(&format!(
                "Data: {name} does not parse, so there is no Node or Territory ({e})"
            ));
        })
        .ok()
}

fn nodes() -> &'static [Node] {
    static NODES: Mutex<Option<(u32, &'static Vec<Node>)>> = Mutex::new(None);
    data::cached::<Vec<Node>>(&NODES, || {
        let (Some(nodes), Some(waypoints), Some(regions), Some(text)) = (
            parse::<Vec<Exploration>>("client/exploration.json"),
            parse::<Waypoints>("client/waypoints.json"),
            parse::<Vec<Region>>("client/regions.json"),
            parse::<Localization>("client/localization.json"),
        ) else {
            return Vec::new();
        };
        let text = |table: &str, id: u32| {
            text.get(table)
                .and_then(|ids| ids.get(&id.to_string()))
                .cloned()
                .unwrap_or_default()
        };
        let main: HashMap<u32, bool> = nodes.iter().map(|n| (n.key, n.main)).collect();
        // Worker sub-nodes are named for the job, like "Mining", so they take
        // the name of the main node they are linked to.
        let parents: HashMap<u32, u32> = waypoints
            .links
            .iter()
            .flat_map(|&(a, b)| [(a, b), (b, a)])
            .filter(|(parent, child)| {
                main.get(parent) == Some(&true) && main.get(child) == Some(&false)
            })
            .map(|(parent, child)| (child, parent))
            .collect();
        let positions: HashMap<u32, [f32; 3]> = waypoints
            .points
            .iter()
            .map(|p| (p.key, p.position))
            .collect();
        nodes
            .iter()
            .filter(|n| n.enabled && n.radius > 0.0)
            .filter_map(|n| {
                // Red Battlefield has no waypoint, only its own anchor.
                let anchor = (n.anchor != [0.0; 3]).then_some(n.anchor);
                let [x, _, z] = positions.get(&n.key).copied().or(anchor)?;
                // No node stores its territory, so it is the nearest region's.
                // Many regions share one point, so a tie goes to the territory
                // most of them belong to.
                let d = |r: &Region| (r.position[0] - x).powi(2) + (r.position[2] - z).powi(2);
                let nearest = regions.iter().map(d).min_by(f32::total_cmp);
                let mut votes: HashMap<u8, usize> = HashMap::new();
                for r in regions.iter().filter(|r| Some(d(r)) == nearest) {
                    *votes.entry(r.territory).or_default() += 1;
                }
                let territory = votes
                    .into_iter()
                    .max_by_key(|&(t, n)| (n, std::cmp::Reverse(t)))
                    .map_or_else(String::new, |(t, _)| text("12", t.into()));
                Some(Node {
                    place: Place {
                        node: text("29", parents.get(&n.key).copied().unwrap_or(n.key)),
                        territory,
                    },
                    x,
                    z,
                    radius: n.radius,
                })
            })
            .collect()
    })
}

/// The smallest node covering the point, or else the one whose edge is
/// nearest, with whether the point is inside it.
pub fn locate(x: f32, z: f32) -> Option<(&'static Place, bool)> {
    let edge = |n: &Node| (n.x - x).hypot(n.z - z) - n.radius;
    nodes()
        .iter()
        .filter(|n| edge(n) <= 0.0)
        .min_by(|a, b| a.radius.total_cmp(&b.radius))
        .map(|n| (&n.place, true))
        .or_else(|| {
            nodes()
                .iter()
                .min_by(|a, b| edge(a).total_cmp(&edge(b)))
                .map(|n| (&n.place, false))
        })
}
