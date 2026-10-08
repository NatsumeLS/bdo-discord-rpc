use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::read::capture;
use crate::{config, http, region};

/// A file's name, its text as this build embeds it, and whether a downloaded
/// copy reads as what the app parses it into.
type Embedded = (&'static str, &'static str, fn(&str) -> bool);

/// Every file the app can download.
const EMBEDDED: &[Embedded] = &[
    (
        "service/regions.toml",
        include_str!("../assets/service/regions.toml"),
        toml_as::<BTreeMap<String, region::Region>>,
    ),
    (
        "service/opcodes.toml",
        include_str!("../assets/service/opcodes.toml"),
        toml_as::<BTreeMap<String, region::Wire>>,
    ),
    (
        "client/exploration.json",
        include_str!("../assets/client/exploration.json"),
        json_as::<Vec<capture::Exploration>>,
    ),
    (
        "client/waypoints.json",
        include_str!("../assets/client/waypoints.json"),
        json_as::<capture::Waypoints>,
    ),
    (
        "client/regions.json",
        include_str!("../assets/client/regions.json"),
        json_as::<Vec<capture::Region>>,
    ),
    (
        "client/localization.json",
        include_str!("../assets/client/localization.json"),
        json_as::<capture::Localization>,
    ),
];
/// Shorter than the profile's, since the capture waits on it.
const TIMEOUT: Duration = Duration::from_secs(5);

fn toml_as<T: DeserializeOwned>(text: &str) -> bool {
    toml::from_str::<T>(text).is_ok()
}

fn json_as<T: DeserializeOwned>(text: &str) -> bool {
    serde_json::from_str::<T>(text).is_ok()
}
const MANIFEST: &str = include_str!("../assets/manifest.json");

/// Bumped by every download, which makes `cached` parse again.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// File name, then the Unix time it last changed.
type Manifest = BTreeMap<String, i64>;

fn manifest(text: &str) -> Manifest {
    serde_json::from_str(text).unwrap_or_default()
}

fn local_manifest() -> Manifest {
    std::fs::read_to_string(config::data_dir().join("manifest.json"))
        .map_or_else(|_| Manifest::new(), |t| manifest(&t))
}

fn url(name: &str) -> String {
    format!(
        "{}/main/assets/{name}",
        env!("CARGO_PKG_REPOSITORY").replacen("github.com", "raw.githubusercontent.com", 1)
    )
}

/// Writes the file into the data folder and lists it there as changed at `at`.
fn save(local: &mut Manifest, name: &str, text: &str, at: i64) -> Result<(), String> {
    config::write_atomic(&config::data_dir().join(name), text)?;
    // Listed per file, so one that fails later keeps the ones before it.
    local.insert(name.to_string(), at);
    let listed = serde_json::to_string_pretty(local).map_err(|e| e.to_string())?;
    config::write_atomic(&config::data_dir().join("manifest.json"), &listed)
}

/// Writes out every embedded file the data folder lacks or holds an older
/// copy of, so the folder holds them all.
pub fn extract() -> Result<(), String> {
    let embedded = manifest(MANIFEST);
    let mut local = local_manifest();
    for &(name, text, _) in EMBEDDED {
        let at = embedded.get(name).copied().unwrap_or_default();
        if local.get(name).is_some_and(|&held| held >= at) && config::data_dir().join(name).exists()
        {
            continue;
        }
        save(&mut local, name, text, at)?;
    }
    Ok(())
}

/// The file's text, from the data folder unless its copy there is older than
/// the one this build embeds.
pub fn load(name: &str) -> Cow<'static, str> {
    if local_manifest().get(name) >= manifest(MANIFEST).get(name) {
        if let Ok(text) = std::fs::read_to_string(config::data_dir().join(name)) {
            return Cow::Owned(text);
        }
    }
    Cow::Borrowed(
        EMBEDDED
            .iter()
            .find(|(n, _, _)| *n == name)
            .map_or("", |(_, text, _)| text),
    )
}

/// Downloads every file the repo has newer than both copies here into the
/// data folder, and returns their names.
pub fn update() -> Result<Vec<&'static str>, String> {
    let remote = manifest(&http::get(&url("manifest.json"), "manifest.json", TIMEOUT)?);
    let embedded = manifest(MANIFEST);
    let mut local = local_manifest();
    let mut updated = Vec::new();
    // Only names this build knows, so the manifest cannot write anywhere else.
    for &(name, _, reads) in EMBEDDED {
        let Some(&at) = remote.get(name) else {
            continue;
        };
        if Some(&at) <= local.get(name).max(embedded.get(name)) {
            continue;
        }
        let text = http::get(&url(name), name, TIMEOUT)?;
        // A newer layout than this build reads would replace a working copy.
        if !reads(&text) {
            return Err(format!("{name} is not in the Layout this Version reads"));
        }
        save(&mut local, name, &text, at)?;
        GENERATION.fetch_add(1, Ordering::Relaxed);
        updated.push(name);
    }
    Ok(updated)
}

/// Parsed once per download, so a new file applies without a restart. The
/// old value is leaked, since borrows of it may still be held.
pub fn cached<T: Sync>(
    slot: &Mutex<Option<(u32, &'static T)>>,
    load: impl FnOnce() -> T,
) -> &'static T {
    let now = GENERATION.load(Ordering::Relaxed);
    let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
    match *slot {
        Some((at, value)) if at == now => value,
        _ => {
            let value = Box::leak(Box::new(load()));
            *slot = Some((now, value));
            value
        }
    }
}
