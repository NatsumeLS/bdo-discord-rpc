use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, MAX_PATH,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

const GAME_EXE: &str = "BlackDesert64.exe";

const STILL_ACTIVE: u32 = 259;

#[derive(Clone)]
pub struct GameProcess {
    pub root: PathBuf,
    pub started_at: Option<SystemTime>,
}

#[derive(Default)]
pub struct GameFinder {
    known: Option<(u32, GameProcess)>,
}

impl GameFinder {
    pub fn find(&mut self) -> Option<GameProcess> {
        if let Some((pid, known)) = &self.known {
            if still_the_game(*pid, known.started_at) {
                return Some(known.clone());
            }
            self.known = None;
        }

        let (pid, exe, started_at) = first_process(GAME_EXE)?;
        let game = GameProcess {
            root: game_root(&exe)?,
            started_at,
        };
        self.known = Some((pid, game.clone()));
        Some(game)
    }
}

fn still_the_game(pid: u32, started_at: Option<SystemTime>) -> bool {
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut code: u32 = 0;
        let alive = GetExitCodeProcess(handle, &mut code) != 0 && code == STILL_ACTIVE;
        // Windows reuses pids, so the start time has to match as well.
        let same = creation_time(handle) == started_at;

        CloseHandle(handle);
        alive && same
    }
}

fn game_root(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let in_bin64 = dir
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("bin64"));
    if in_bin64 {
        dir.parent().map(Path::to_path_buf)
    } else {
        Some(dir.to_path_buf())
    }
}

fn first_process(exe_name: &str) -> Option<(u32, PathBuf, Option<SystemTime>)> {
    unsafe {
        let snapshot: HANDLE = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut found = None;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            if wide_to_string(&entry.szExeFile).eq_ignore_ascii_case(exe_name) {
                let pid = entry.th32ProcessID;
                if let Some((path, started_at)) = details_of(pid) {
                    found = Some((pid, path, started_at));
                    break;
                }
            }
            ok = Process32NextW(snapshot, &mut entry);
        }

        CloseHandle(snapshot);
        found
    }
}

unsafe fn details_of(pid: u32) -> Option<(PathBuf, Option<SystemTime>)> {
    let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return None;
    }

    let path = image_path(handle);
    let started_at = creation_time(handle);

    CloseHandle(handle);
    path.map(|path| (path, started_at))
}

unsafe fn image_path(handle: HANDLE) -> Option<PathBuf> {
    let mut buf = [0u16; MAX_PATH as usize * 2];
    let mut size = buf.len() as u32;
    if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) == 0 || size == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &buf[..size as usize],
    )))
}

unsafe fn creation_time(handle: HANDLE) -> Option<SystemTime> {
    let mut creation: FILETIME = std::mem::zeroed();
    let mut exit: FILETIME = std::mem::zeroed();
    let mut kernel: FILETIME = std::mem::zeroed();
    let mut user: FILETIME = std::mem::zeroed();
    if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
        return None;
    }

    const EPOCH_DIFF_100NS: u64 = 11_644_473_600 * 10_000_000;
    let ticks = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
    let since_unix = ticks.checked_sub(EPOCH_DIFF_100NS)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(since_unix.checked_mul(100)?))
}

fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

pub fn detect_service(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("service.ini")).ok()?;

    let mut in_service = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_service = line.eq_ignore_ascii_case("[SERVICE]");
            continue;
        }
        if !in_service {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("type") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

pub fn default_user_data_dir() -> Option<PathBuf> {
    dirs::document_dir().map(|d| d.join("Black Desert"))
}

pub fn detect_family(user_data_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(user_data_dir.join("UserCache")).ok()?;

    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (e.file_name().to_string_lossy().into_owned(), modified)
        })
        .filter(|(name, _)| {
            !name.is_empty() && name != "-1" && !name.chars().all(|c| c.is_ascii_digit())
        })
        .max_by_key(|&(_, modified)| modified)
        .map(|(name, _)| name)
}
pub const CHARACTER_READ_WINDOW: Duration = Duration::from_secs(10);

// Each character's folder, as its ID and path.
fn characters(user_data_dir: &Path) -> impl Iterator<Item = (String, PathBuf)> {
    let children = |dir: PathBuf| std::fs::read_dir(dir).into_iter().flatten().flatten();

    children(user_data_dir.join("UserCache"))
        .flat_map(move |account| children(account.path()))
        .flat_map(move |region| children(region.path()))
        .filter_map(|character| {
            let name = character.file_name().to_string_lossy().into_owned();
            (!name.is_empty() && name.chars().all(|c| c.is_ascii_digit()))
                .then(|| (name, character.path()))
        })
}

pub fn character_ids(user_data_dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = characters(user_data_dir).map(|(id, _)| id).collect();
    ids.sort();
    ids.dedup();
    ids
}

pub fn detect_character(user_data_dir: &Path, play_started: SystemTime) -> Option<String> {
    characters(user_data_dir)
        .filter_map(|(name, path)| {
            // Accessed, not modified: the client reads this file on entering
            // a character and writes it on leaving, so the newest write is
            // always the previous character.
            let read_at = std::fs::metadata(path.join("gamevariable.xml"))
                .and_then(|m| m.accessed())
                .ok()?;
            let gap = read_at
                .duration_since(play_started)
                .unwrap_or_else(|e| e.duration());
            (gap <= CHARACTER_READ_WINDOW).then_some((name, gap))
        })
        .min_by_key(|&(_, gap)| gap)
        .map(|(name, _)| name)
}
