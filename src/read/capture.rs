use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::Local;
use pcap::{Capture, Device};
use serde::Deserialize;

use windows_sys::Win32::System::LibraryLoader::{LoadLibraryW, SetDllDirectoryW};

use crate::phase::Phase;
use crate::read::{game, log_tail};
use crate::region::{self, Wire};
use crate::watch::Shared;
use crate::win::{self, log};

const HEADER: usize = 5;
/// The world server's port, which carries character select and play.
const WORLD_PORT: u16 = 8889;
/// World server hosts tried under the log's domain, `game01` up.
const SERVER_COUNT: u32 = 99;
const SYNC_CHAIN: usize = 3;
const MAX_PENDING: usize = 256;

// The game's own tables, as tools/dump.rs writes them to assets/game.
#[derive(Deserialize)]
struct Exploration {
    key: u32,
    enabled: bool,
    main: bool,
    radius: f32,
    anchor: [f32; 3],
}

#[derive(Deserialize)]
struct Waypoints {
    points: Vec<Waypoint>,
    links: Vec<(u32, u32)>,
}

#[derive(Deserialize)]
struct Waypoint {
    key: u32,
    position: [f32; 3],
}

#[derive(Deserialize)]
struct Region {
    territory: u8,
    position: [f32; 3],
}

/// Table, then id, then its text: 12 territory names, 29 node names.
type Localization = HashMap<String, HashMap<String, String>>;

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

fn nodes() -> &'static [Node] {
    static NODES: OnceLock<Vec<Node>> = OnceLock::new();
    NODES.get_or_init(|| {
        let (Ok(nodes), Ok(waypoints), Ok(regions), Ok(text)) = (
            serde_json::from_str::<Vec<Exploration>>(include_str!(
                "../../assets/game/exploration.json"
            )),
            serde_json::from_str::<Waypoints>(include_str!("../../assets/game/waypoints.json")),
            serde_json::from_str::<Vec<Region>>(include_str!("../../assets/game/regions.json")),
            serde_json::from_str::<Localization>(include_str!(
                "../../assets/game/localization.json"
            )),
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
                let territory = regions
                    .iter()
                    .min_by(|a, b| {
                        let d =
                            |r: &Region| (r.position[0] - x).powi(2) + (r.position[2] - z).powi(2);
                        d(a).total_cmp(&d(b))
                    })
                    .map_or_else(String::new, |r| text("12", r.territory.into()));
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
fn locate(x: f32, z: f32) -> Option<(&'static Place, bool)> {
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

// Bounds checked, since the offsets come from regions.toml.
fn f32_at(f: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(f.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(f: &[u8], at: usize) -> u64 {
    f.get(at..at + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}

fn utf16_at(f: &[u8], at: usize, max: usize) -> String {
    let units: Vec<u16> = f
        .get(at..(at + max * 2).min(f.len()))
        .unwrap_or_default()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}
const RECHECK: Duration = Duration::from_secs(5);
const STATS_EVERY: Duration = Duration::from_secs(10);

/// Loads Npcap's wpcap.dll, which every pcap call needs first: it is delay
/// loaded, and a call without it crashes. False when Npcap is not installed.
fn npcap() -> bool {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    // Npcap keeps it outside the search path, beside the Packet.dll it loads.
    unsafe {
        SetDllDirectoryW(wide(&format!(r"{root}\System32\Npcap")).as_ptr());
        !LoadLibraryW(wide("wpcap.dll").as_ptr()).is_null()
    }
}

/// The address the default route leaves from. Connecting UDP sends nothing.
fn default_local_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:53").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

struct Segment<'a> {
    src: SocketAddrV4,
    dst: SocketAddrV4,
    seq: u32,
    syn: bool,
    /// FIN or RST: the connection is ending.
    end: bool,
    payload: &'a [u8],
}

fn tcp_segment(frame: &[u8], linktype: i32) -> Option<Segment<'_>> {
    let ip = match linktype {
        1 => {
            let mut at = 12;
            let mut ether = u16::from_be_bytes([*frame.get(at)?, *frame.get(at + 1)?]);
            if ether == 0x8100 {
                at += 4;
                ether = u16::from_be_bytes([*frame.get(at)?, *frame.get(at + 1)?]);
            }
            if ether != 0x0800 {
                return None;
            }
            frame.get(at + 2..)?
        }
        0 | 108 => frame.get(4..)?,
        12 | 101 => frame,
        _ => return None,
    };
    if ip.first()? >> 4 != 4 || *ip.get(9)? != 6 {
        return None;
    }
    let header = (ip[0] & 0x0F) as usize * 4;
    // Zero under large send offload, where the NIC fills it in later.
    let total = match u16::from_be_bytes([*ip.get(2)?, *ip.get(3)?]) as usize {
        0 => ip.len(),
        n => n.min(ip.len()),
    };
    let src_ip = Ipv4Addr::from(<[u8; 4]>::try_from(ip.get(12..16)?).ok()?);
    let dst_ip = Ipv4Addr::from(<[u8; 4]>::try_from(ip.get(16..20)?).ok()?);
    let tcp = ip.get(header..total)?;
    let port = |at: usize| Some(u16::from_be_bytes([*tcp.get(at)?, *tcp.get(at + 1)?]));
    let seq = u32::from_be_bytes(tcp.get(4..8)?.try_into().ok()?);
    let offset = (*tcp.get(12)? >> 4) as usize * 4;
    Some(Segment {
        src: SocketAddrV4::new(src_ip, port(0)?),
        dst: SocketAddrV4::new(dst_ip, port(2)?),
        seq,
        syn: tcp.get(13)? & 0x02 != 0,
        end: tcp[13] & 0x05 != 0,
        payload: tcp.get(offset..)?,
    })
}

#[derive(Default)]
struct Stream {
    // Only when printing.
    name: Option<String>,
    next_seq: Option<u32>,
    buf: Vec<u8>,
    pending: BTreeMap<u32, Vec<u8>>,
    synced: bool,
    segments: u32,
    bytes: u64,
    frames: u64,
    dropped: u64,
    reported: u64,
}

impl Stream {
    fn feed(&mut self, seg: &Segment, mut emit: impl FnMut(&[u8])) {
        if seg.syn {
            // From the very first byte, so the framing needs no guessing.
            *self = Stream {
                name: std::mem::take(&mut self.name),
                next_seq: Some(seg.seq.wrapping_add(1)),
                synced: true,
                ..Stream::default()
            };
            return;
        }
        if seg.payload.is_empty() {
            return;
        }
        let next = *self.next_seq.get_or_insert(seg.seq);
        if seg.seq.wrapping_sub(next) as i32 > 0 {
            self.pending.insert(seg.seq, seg.payload.to_vec());
            if self.pending.len() <= MAX_PENDING {
                return;
            }
            // Waited long enough: the missing bytes are gone, and so is the
            // frame boundary.
            let first = *self.pending.keys().next().unwrap_or(&seg.seq);
            self.dropped += self.buf.len() as u64;
            self.buf.clear();
            self.synced = false;
            self.next_seq = Some(first);
        } else {
            self.take(seg.seq, seg.payload, &mut emit);
        }
        while let Some(&seq) = self.pending.keys().next() {
            let next = self.next_seq.unwrap_or(seq);
            if seq.wrapping_sub(next) as i32 > 0 {
                break;
            }
            let data = self.pending.remove(&seq).unwrap_or_default();
            self.take(seq, &data, &mut emit);
        }
    }

    fn take(&mut self, seq: u32, payload: &[u8], emit: &mut impl FnMut(&[u8])) {
        let behind = self.next_seq.unwrap_or(seq).wrapping_sub(seq) as usize;
        let fresh = match payload.get(behind..) {
            Some(rest) if !rest.is_empty() => rest,
            _ => return,
        };
        self.next_seq = Some(seq.wrapping_add(payload.len() as u32));
        self.segments += 1;
        self.bytes += fresh.len() as u64;
        if let Some(name) = self.name.as_ref().filter(|_| self.segments <= 3) {
            println!(
                "           first bytes {name}: {}",
                hex(&fresh[..fresh.len().min(32)])
            );
        }
        self.buf.extend_from_slice(fresh);

        loop {
            if !self.synced {
                match find_sync(&self.buf) {
                    Some(at) => {
                        self.dropped += at as u64;
                        self.buf.drain(..at);
                        self.synced = true;
                    }
                    None => {
                        let keep = self
                            .buf
                            .len()
                            .saturating_sub(u16::MAX as usize * SYNC_CHAIN);
                        self.dropped += keep as u64;
                        self.buf.drain(..keep);
                        return;
                    }
                }
            }
            let Some(len) = frame_len(&self.buf, 0) else {
                if self.buf.len() >= HEADER {
                    self.synced = false;
                    self.dropped += 1;
                    self.buf.drain(..1);
                    continue;
                }
                return;
            };
            if self.buf.len() < len {
                return;
            }
            emit(&self.buf[..len]);
            self.frames += 1;
            self.buf.drain(..len);
        }
    }
}

// Assumes a little-endian u16 length that counts the header itself.
fn frame_len(buf: &[u8], at: usize) -> Option<usize> {
    let len = u16::from_le_bytes([*buf.get(at)?, *buf.get(at + 1)?]) as usize;
    (len >= HEADER && buf.len() >= at + HEADER).then_some(len)
}

fn find_sync(buf: &[u8]) -> Option<usize> {
    (0..buf.len()).find(|&start| {
        let mut at = start;
        for _ in 0..SYNC_CHAIN {
            match frame_len(buf, at) {
                Some(len) if at + len <= buf.len() && buf[at + 2] <= 1 => at += len,
                _ => return false,
            }
        }
        true
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

enum Event {
    Place(Option<(&'static Place, bool)>, f32, f32),
    /// A character entered the world on the server at this address.
    Entered {
        server: Ipv4Addr,
        id: u64,
        name: String,
        family: String,
    },
    /// The character list, sent at character select and on a channel switch.
    Listed,
    /// The world connection the character entered on has closed.
    Left,
    Listening,
    /// Started with the game already connected to the world server at this
    /// address, so no entry was seen.
    Joined(Ipv4Addr),
    /// The game closed or restarted, so nothing captured from it still holds.
    GameEnded,
    /// Once a second, so waiting work is retried while the game sends nothing.
    Tick,
}

/// How the tray's capture is doing.
#[derive(Clone, Default, PartialEq)]
pub enum CaptureState {
    #[default]
    Starting,
    NoNpcap,
    Failed(String),
    /// Listening since this Unix time.
    Listening(i64),
}

/// What the capture knows, which the tray prefers over what it reads from
/// disk. Everything here is cleared when the game comes or goes.
#[derive(Clone, Default)]
pub struct Captured {
    pub place: Place,
    /// The world server's key, like `game37.sg`.
    pub server: Option<String>,
    /// On a world server not yet named, so the log's server is not to be
    /// trusted either.
    pub resolving: bool,
    /// The character's ID, the same number as its `UserCache` folder, and
    /// its name in game.
    pub character: Option<(String, String)>,
    pub family: Option<String>,
    /// The phase and the Unix time it began.
    pub phase: Option<(Phase, i64)>,
    /// When a character last entered the world, in Unix time.
    pub entered: Option<i64>,
    /// Started with the game already in the world, so it saw no entry.
    pub joined: bool,
}

/// Runs until killed, printing every game frame. Safe to start before the game.
pub fn run(dump: Option<&str>) -> i32 {
    if !npcap() {
        eprintln!("Capture: Npcap is not installed, get it from https://npcap.com");
        return 1;
    }
    let printed = capture(dump, true, |event| match event {
        Event::Place(here, x, z) => {
            let name = match here {
                Some((place, true)) => format!("{} - {}", place.territory, place.node),
                Some((place, false)) => format!("{} - near {}", place.territory, place.node),
                None => "(no nodes loaded)".into(),
            };
            println!("  >> location {name} at {x:.0}, {z:.0}");
        }
        Event::Entered {
            server,
            id,
            name,
            family,
            ..
        } => println!("  >> entered {name} of {family} ({id}) on {server}"),
        Event::Listed => println!("  >> character list"),
        Event::Left => println!("  >> left the world server"),
        Event::Listening => {}
        Event::Joined(server) => println!("  >> already on the world server {server}"),
        Event::GameEnded => println!("  >> game closed"),
        Event::Tick => {}
    });
    match printed {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Capture: {e}");
            1
        }
    }
}

/// Keeps the tray's `Captured` current, until the capture fails.
pub fn follow(shared: &Shared) {
    let set = |state| {
        if let Ok(mut capture) = shared.capture.lock() {
            *capture = state;
        }
    };
    if !npcap() {
        win::warn("Capture: Npcap is not installed, falling back to the Game Files");
        set(CaptureState::NoNpcap);
        return;
    }
    let mut world = None;
    let mut named = None;
    let mut hosts: (String, HashMap<Ipv4Addr, String>) = Default::default();
    let followed = capture(None, false, |event| {
        let Ok(mut captured) = shared.captured.lock() else {
            return;
        };
        let now = Local::now().timestamp();
        match event {
            Event::Listening => set(CaptureState::Listening(now)),
            Event::Joined(server) => {
                world = Some(server);
                captured.joined = true;
            }
            Event::GameEnded => {
                world = None;
                named = None;
                *captured = Captured::default();
            }
            Event::Tick => {}
            Event::Place(here, _, _) => {
                captured.place = here.map(|(found, _)| found.clone()).unwrap_or_default();
            }
            Event::Entered {
                server,
                id,
                name,
                family,
            } => {
                world = Some(server);
                captured.character = Some((id.to_string(), name));
                captured.family = (!family.is_empty()).then_some(family);
                captured.phase = Some((Phase::Play, now));
                captured.entered = Some(now);
            }
            Event::Listed => captured.phase = Some((Phase::Lobby, now)),
            // Whatever the character was doing there is over, so nothing
            // read on that connection still holds.
            Event::Left => {
                world = None;
                named = None;
                captured.place = Place::default();
                captured.server = None;
                captured.character = None;
                captured.phase = Some((Phase::Loading, now));
            }
        }
        // Retried every event and tick, since the entry can arrive before the
        // watcher has read the domain from the log.
        captured.resolving = world.is_some() && world != named;
        if world == named {
            return;
        }
        // Resolving takes seconds, which the watcher must not wait out.
        drop(captured);
        let Some(domain) = shared.server_domain.lock().ok().and_then(|d| d.clone()) else {
            return;
        };
        let Some(ip) = world else {
            return;
        };
        // An address the table lacks may be a lookup that failed, so it is
        // tried again, once per entry.
        if hosts.0 != domain || !hosts.1.contains_key(&ip) {
            hosts = (domain.clone(), game_servers(&domain));
        }
        named = world;
        let server = hosts.1.get(&ip).cloned();
        if server.is_none() {
            win::warn(&format!("Capture: {ip} is no game{{nn}}.{domain} server"));
        }
        if let Ok(mut captured) = shared.captured.lock() {
            captured.server = server;
            captured.resolving = false;
        }
    });
    if let Err(e) = followed {
        win::warn(&format!("Capture: {e}, falling back to the Game Files"));
        set(CaptureState::Failed(e));
    }
}

/// Every world server host under the domain, by address. The servers publish
/// no reverse DNS, so the names are looked up forward and matched.
fn game_servers(domain: &str) -> HashMap<Ipv4Addr, String> {
    (1..=SERVER_COUNT)
        .filter_map(|n| {
            let host = format!("game{n:02}.{domain}");
            let ip = (host.as_str(), 0)
                .to_socket_addrs()
                .ok()?
                .find_map(|a| match a.ip() {
                    IpAddr::V4(ip) => Some(ip),
                    IpAddr::V6(_) => None,
                })?;
            Some((ip, log_tail::server_key(&host)))
        })
        .collect()
}

fn note(verbose: bool, text: &str) {
    if verbose {
        println!("{text}");
    } else {
        win::warn(text);
    }
}

/// The game's capture keys, or none without any, and then the game files
/// stand in.
fn wire_of(process: &game::GameProcess, verbose: bool) -> Option<&'static Wire> {
    let client = game::client_version(&process.root);
    let Some((build, wire)) = region::wire(client) else {
        note(
            verbose,
            "Capture: opcodes.toml has no Opcodes, falling back to the Game Files",
        );
        return None;
    };
    // Still decoded, since most patches leave the messages where they were.
    if let Some(client) = client.filter(|&client| client != build) {
        note(
            verbose,
            &format!(
                "Capture: The Client is version {client} and the Capture Keys are for {build}, so the Opcodes may have moved"
            ),
        );
    }
    Some(wire)
}

// Only after `npcap()`, since every pcap call before it crashes.
fn capture(dump: Option<&str>, verbose: bool, mut on: impl FnMut(Event)) -> Result<(), String> {
    let mut dump = match dump {
        Some(path) => Some(std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?),
        None => None,
    };
    let local = default_local_ip().ok_or("No IPv4 default route")?;
    let device = Device::list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.addresses.iter().any(|a| a.addr == IpAddr::V4(local)))
        .ok_or_else(|| format!("No capture device has the address {local}"))?;
    let name = device.name.clone();
    let mut cap = Capture::from_device(device)
        .and_then(|c| {
            c.promisc(false)
                .snaplen(65535)
                .timeout(500)
                .buffer_size(16 << 20)
                .open()
        })
        .map_err(|e| e.to_string())?;
    cap.filter("tcp", true).map_err(|e| e.to_string())?;
    let linktype = cap.get_datalink().0;
    if verbose {
        println!("Capturing on {name} ({local}), link type {linktype}");
        println!("Waiting for {}", game::GAME_EXE);
    } else {
        log(&format!("Capture: Listening on {local}"));
    }
    on(Event::Listening);

    let mut finder = game::GameFinder::default();
    let identity = |p: &Option<game::GameProcess>| p.as_ref().map(|p| (p.pid, p.started_at));
    let mut running = finder.find();
    // What to decode for the running game's build, looked up again for each game.
    let mut wire_for = identity(&running);
    let mut wire = running.as_ref().and_then(|p| wire_of(p, verbose));
    // The connection the character entered on, whose end ends what it said.
    // Started in the world, the game already holds it, which names the
    // server now, while the character waits for the next entry.
    let mut world = running
        .as_ref()
        .filter(|_| wire.is_some())
        .and_then(|process| {
            game::connections(process.pid)
                .into_iter()
                .find(|(_, remote, _)| remote.port() == WORLD_PORT)
                .map(|(local, remote, _)| (local, remote))
        });
    if let Some((_, remote)) = world {
        note(
            verbose,
            "Capture: Started while in the World, falling back to the Game Files for the Character until the next Load-in",
        );
        on(Event::Joined(*remote.ip()));
    }
    // Keyed by (local, remote), the value says whether the game owns it.
    let mut owners: HashMap<(SocketAddrV4, SocketAddrV4), (bool, Instant)> = HashMap::new();
    let mut streams: HashMap<(SocketAddrV4, SocketAddrV4), Stream> = HashMap::new();
    let started = Instant::now();
    let mut stats_at = Instant::now();
    let mut place: Option<(&Place, bool)> = None;
    let mut ticked = Instant::now();

    loop {
        if ticked.elapsed() >= Duration::from_secs(1) {
            ticked = Instant::now();
            // Only a game already found is cheap to check. A new one is
            // found by its first connection.
            if running.is_some() {
                let now = finder.find();
                if identity(&now) != identity(&running) {
                    running = now;
                    world = None;
                    place = None;
                    on(Event::GameEnded);
                }
            }
            owners.retain(|_, &mut (game, at)| game || at.elapsed() < RECHECK);
            on(Event::Tick);
        }
        let packet = match cap.next_packet() {
            Ok(packet) => packet,
            Err(pcap::Error::TimeoutExpired) => continue,
            Err(e) => return Err(e.to_string()),
        };
        if verbose && stats_at.elapsed() >= STATS_EVERY {
            stats_at = Instant::now();
            for ((src, dst), s) in streams.iter_mut().filter(|(_, s)| s.bytes != s.reported) {
                s.reported = s.bytes;
                println!(
                    "  stats {src} -> {dst}: {} bytes, {} frames, {} dropped, {}",
                    s.bytes,
                    s.frames,
                    s.dropped,
                    if s.synced { "synced" } else { "unsynced" }
                );
            }
        }

        let Some(seg) = tcp_segment(packet.data, linktype) else {
            continue;
        };
        let outbound = seg.src.ip() == &local;
        let key = if outbound {
            (seg.src, seg.dst)
        } else {
            (seg.dst, seg.src)
        };

        let known = owners.get(&key).copied();
        let is_game = match known {
            Some((game, at)) if game || at.elapsed() < RECHECK => game,
            _ => {
                let process = finder.find();
                let game = process.as_ref().is_some_and(|process| {
                    game::connections(process.pid)
                        .iter()
                        .any(|&(l, r, _)| (l, r) == key)
                });
                if running.is_none() {
                    running = process;
                }
                if verbose && game && known.is_none_or(|(was, _)| !was) {
                    println!(
                        "{:>9.3}  connection {} -> {}",
                        started.elapsed().as_secs_f64(),
                        key.0,
                        key.1
                    );
                }
                owners.insert(key, (game, Instant::now()));
                game
            }
        };
        if !is_game {
            continue;
        }
        if identity(&running) != wire_for {
            wire_for = identity(&running);
            wire = running.as_ref().and_then(|p| wire_of(p, verbose));
        }

        if seg.end && world == Some(key) {
            world = None;
            place = None;
            on(Event::Left);
        }

        let dir = if outbound { "out" } else { "in " };
        let remote = key.1;
        let at = started.elapsed().as_secs_f64();
        let mut entered = false;
        streams
            .entry((seg.src, seg.dst))
            .or_insert_with(|| Stream {
                name: verbose.then(|| format!("{} -> {}", seg.src, seg.dst)),
                ..Stream::default()
            })
            .feed(&seg, |f| {
                let opcode = u16::from_le_bytes([f[3], f[4]]);
                if verbose {
                    let body = &f[HEADER..];
                    println!(
                        "{at:>9.3}  {dir} {remote:<21}  op {opcode:04X}  flag {:02X}  len {:>5}  {}",
                        f[2],
                        f.len(),
                        hex(&body[..body.len().min(24)])
                    );
                }
                if let Some(file) = dump.as_mut() {
                    let _ = writeln!(file, "{at:.3} {} {remote} {}", dir.trim(), hex(f));
                }
                let Some(w) = wire.filter(|_| !outbound && f[2] == 0) else {
                    return;
                };
                let offset = match opcode {
                    op if op == w.enter && f.len() == w.enter_length => {
                        entered = true;
                        on(Event::Entered {
                            server: *remote.ip(),
                            id: u64_at(f, w.enter_id),
                            name: utf16_at(f, w.enter_name, 16),
                            family: utf16_at(f, w.enter_family, 16),
                        });
                        w.enter_position
                    }
                    op if op == w.list => {
                        on(Event::Listed);
                        return;
                    }
                    op if op == w.position => w.position_at,
                    _ => return,
                };
                let (Some(x), Some(z)) = (f32_at(f, offset), f32_at(f, offset + 8)) else {
                    return;
                };
                let here = locate(x, z);
                if here != place {
                    place = here;
                    on(Event::Place(here, x, z));
                }
            });
        if entered {
            world = Some(key);
        }
        if seg.end {
            streams.remove(&(seg.src, seg.dst));
            owners.remove(&key);
        }
    }
}
