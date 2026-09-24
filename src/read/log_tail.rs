use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{Local, NaiveDateTime, TimeZone};

use crate::phase::Phase;

const RESCAN_INTERVAL: Duration = Duration::from_secs(10);
const READ_CHUNK: usize = 64 * 1024;
const MAX_PENDING: usize = 1024 * 1024;
const GAME_SERVER_MARKER: &str = "I will connect to 4-server type(";

#[derive(Default)]
pub struct GameState {
    pub phase: Option<Phase>,
    pub phase_since: Option<i64>,
    pub session_start: Option<i64>,
    pub game_server: Option<String>,
}

pub struct LogTail {
    log_dir: PathBuf,
    current: Option<PathBuf>,
    offset: u64,
    pending: String,
    carry: Option<u8>,
    last_scan: Option<Instant>,
    not_before: Option<SystemTime>,
}

impl LogTail {
    pub fn new(game_root: &Path, not_before: Option<SystemTime>) -> Self {
        LogTail {
            log_dir: game_root.join("Log"),
            current: None,
            offset: 0,
            pending: String::new(),
            carry: None,
            last_scan: None,
            not_before,
        }
    }

    pub fn file_name(&self) -> Option<String> {
        let name = self.current.as_deref()?.file_name()?;
        Some(name.to_string_lossy().into_owned())
    }

    fn select_newest(&mut self) {
        let due = self.current.is_none()
            || self
                .last_scan
                .is_none_or(|last| last.elapsed() >= RESCAN_INTERVAL);
        if !due {
            return;
        }
        self.last_scan = Some(Instant::now());

        let Ok(entries) = std::fs::read_dir(&self.log_dir) else {
            return;
        };

        let newest = entries
            .flatten()
            .filter_map(|entry| {
                if !is_client_log(&entry.file_name()) {
                    return None;
                }
                let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
                match self.not_before {
                    Some(floor) if modified < floor => None,
                    _ => Some((entry.path(), modified)),
                }
            })
            .max_by_key(|&(_, modified)| modified)
            .map(|(path, _)| path);

        if let Some(path) = newest {
            if self.current.as_deref() != Some(path.as_path()) {
                self.current = Some(path);
                self.rewind();
            }
        }
    }

    fn rewind(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.carry = None;
    }

    pub fn poll(&mut self, state: &mut GameState) {
        self.select_newest();
        let Some(path) = self.current.clone() else {
            return;
        };

        let Ok(mut file) = File::open(&path) else {
            return;
        };
        let Ok(size) = file.metadata().map(|m| m.len()) else {
            return;
        };

        if size < self.offset {
            self.rewind();
        }
        if size == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }

        let mut buffer = vec![0u8; READ_CHUNK];
        loop {
            let read = match file.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            self.offset += read as u64;

            let mut bytes = Vec::with_capacity(read + 1);
            bytes.extend(self.carry.take());
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.len() % 2 == 1 {
                self.carry = bytes.pop();
            }

            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_le_bytes(pair))
                .collect();
            self.pending.push_str(&String::from_utf16_lossy(&units));

            while let Some(newline) = self.pending.find('\n') {
                let line = self.pending[..newline].trim_end_matches('\r').to_string();
                self.pending.drain(..=newline);
                apply_record(&line, state);
            }

            if self.pending.len() > MAX_PENDING {
                self.pending.clear();
            }
        }
    }
}

fn is_client_log(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.starts_with("Client_") && n.ends_with(".json"))
}

fn extract(record: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = record.find(&needle)? + needle.len();

    let mut out = String::new();
    let mut chars = record[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return None,
            },
            '"' => return Some(out),
            other => out.push(other),
        }
    }
    None
}

fn parse_log_date(text: &str) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(text.get(..19)?, "%Y-%m-%d %H:%M:%S").ok()?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp())
}

fn apply_record(record: &str, state: &mut GameState) {
    let Some(message) = extract(record, "Log") else {
        return;
    };

    let stamp = extract(record, "Date")
        .and_then(|d| parse_log_date(&d))
        .unwrap_or_else(|| Local::now().timestamp());

    if state.session_start.is_none() {
        state.session_start = Some(stamp);
    }

    if let Some(rest) = message.strip_prefix("Active Processor (eProcessor_") {
        if let Some(name) = rest.strip_suffix(')') {
            if let Some(phase) = Phase::from_processor(name) {
                if state.phase != Some(phase) {
                    state.phase = Some(phase);
                    state.phase_since = Some(stamp);
                }
            }
        }
        return;
    }

    if let Some(at) = message.find(GAME_SERVER_MARKER) {
        let rest = &message[at + GAME_SERVER_MARKER.len()..];
        let host = &rest[..rest.find([':', ')']).unwrap_or(rest.len())];
        if !host.is_empty() {
            state.game_server = Some(server_key(host));
        }
    }
}

pub fn server_key(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() > 2 {
        labels[..labels.len() - 2].join(".")
    } else {
        host.to_string()
    }
}
