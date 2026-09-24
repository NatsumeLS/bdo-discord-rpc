use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Local;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS,
    INVALID_HANDLE_VALUE, TRUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, SetEvent, WaitForSingleObject,
    EVENT_MODIFY_STATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub const MESSAGE_AT: usize = 26;

    pub fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }

    pub fn of(line: &str) -> Option<Level> {
        let tag = line.get(20..25)?;
        [Level::Info, Level::Warn, Level::Error]
            .into_iter()
            .find(|level| level.tag() == tag)
    }
}

pub fn log(message: &str) {
    write(Level::Info, message);
}

pub fn warn(message: &str) {
    write(Level::Warn, message);
}

pub fn error(message: &str) {
    write(Level::Error, message);
}

fn write(level: Level, message: &str) {
    let now = Local::now();
    println!("{} {} {message}", now.format("%H:%M:%S"), level.tag());
    append(&format!(
        "{} {} {message}",
        now.format("%Y-%m-%d %H:%M:%S"),
        level.tag()
    ));
}

pub fn attach_console() {
    unsafe {
        let handle: *mut c_void = GetStdHandle(STD_OUTPUT_HANDLE);
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != TRUE {
            return;
        }

        let Ok(conout) = OpenOptions::new().write(true).open("CONOUT$") else {
            return;
        };

        // Leaked on purpose, the handle has to live until the process exits.
        let handle = conout.into_raw_handle();
        SetStdHandle(STD_OUTPUT_HANDLE, handle);
        SetStdHandle(STD_ERROR_HANDLE, handle);
    }
}

const MAX_BYTES: u64 = 10 * 1024 * 1024;

static SINK: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn enable_logfile(path: PathBuf) {
    rotate_if(&path, 1);
    if let Ok(mut sink) = SINK.lock() {
        *sink = Some(path);
    }
}

fn append(line: &str) {
    let Ok(sink) = SINK.lock() else {
        return;
    };
    let Some(path) = sink.as_deref() else {
        return;
    };

    rotate_if(path, MAX_BYTES);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn rotate_if(path: &Path, min_bytes: u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.len() >= min_bytes {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
}

#[derive(Default)]
pub struct ChildWindow {
    child: Option<Child>,
    args: String,
}

impl ChildWindow {
    pub fn is_open(&mut self) -> bool {
        match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => true,
            Some(exited) => {
                if let Ok(Some(status)) = exited {
                    if !status.success() {
                        warn(&format!("{} closed with {status}", self.args));
                    }
                }
                self.child = None;
                false
            }
            None => false,
        }
    }

    pub fn open(&mut self, args: &[&str]) -> std::io::Result<()> {
        if !self.is_open() {
            let child = std::process::Command::new(std::env::current_exe()?)
                .args(args)
                .spawn()?;
            self.child = Some(child);
            self.args = args.join(" ");
        }
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "BdoDiscordRpc";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key(access: u32) -> Option<HKEY> {
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            access,
            &mut key,
        )
    };
    (status == ERROR_SUCCESS).then_some(key)
}

fn startup_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find our Path: {e}"))?;
    Ok(format!("\"{}\"", exe.display()))
}

fn startup_value() -> Option<String> {
    let key = open_run_key(KEY_READ)?;

    let mut size: u32 = 0;
    let mut status = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };

    // Room for a terminator the stored value may not have.
    let mut buffer = vec![0u16; size as usize / 2 + 1];
    if status == ERROR_SUCCESS {
        size = (buffer.len() * 2) as u32;
        status = unsafe {
            RegQueryValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                buffer.as_mut_ptr() as *mut u8,
                &mut size,
            )
        };
    }
    unsafe { RegCloseKey(key) };

    (status == ERROR_SUCCESS).then(|| {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    })
}

pub fn startup_enabled() -> bool {
    let Some(value) = startup_value() else {
        return false;
    };

    if startup_command().is_ok_and(|current| current != value) {
        let _ = set_startup(true);
    }

    true
}

pub fn set_startup(enabled: bool) -> Result<(), String> {
    let key = open_run_key(KEY_READ | KEY_WRITE)
        .ok_or_else(|| "Could not open the Run Key".to_string())?;

    let status = if enabled {
        let value = wide(&startup_command()?);
        unsafe {
            RegSetValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr() as *const u8,
                // Bytes, including the terminator, not code units.
                (value.len() * 2) as u32,
            )
        }
    } else {
        let status = unsafe { RegDeleteValueW(key, wide(VALUE_NAME).as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            ERROR_SUCCESS
        } else {
            status
        }
    };

    unsafe { RegCloseKey(key) };

    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Registry write failed with Code {status}"))
    }
}

pub const TRAY: &str = "BdoDiscordRpc.Tray";
pub const SETTINGS: &str = "BdoDiscordRpc.Settings";
pub const PROMPT: &str = "BdoDiscordRpc.Prompt";

// Held until exit, since closing the handle would release the mutex.
static INSTANCE: Mutex<Option<usize>> = Mutex::new(None);

pub fn claim_instance(name: &str) -> bool {
    let scoped = wide(&format!(r"Local\{name}"));
    let handle = unsafe { CreateMutexW(std::ptr::null(), TRUE, scoped.as_ptr()) };
    if handle.is_null() {
        // Refusing to open a window is worse than opening a second one.
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return false;
    }
    if let Ok(mut slot) = INSTANCE.lock() {
        *slot = Some(handle as usize);
    }
    true
}

const CONFIG_EVENT: &str = r"Local\BdoDiscordRpc.ConfigChanged";

// Held until exit, or the event is destroyed and a window's signal is lost.
static CONFIG_CHANGED: Mutex<Option<usize>> = Mutex::new(None);

pub fn listen_for_config_changes() {
    let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, wide(CONFIG_EVENT).as_ptr()) };
    if !handle.is_null() {
        if let Ok(mut slot) = CONFIG_CHANGED.lock() {
            *slot = Some(handle as usize);
        }
    }
}

pub fn sleep_until_config_changes(timeout: Duration) -> bool {
    let Some(handle) = CONFIG_CHANGED.lock().ok().and_then(|slot| *slot) else {
        std::thread::sleep(timeout);
        return false;
    };
    let waited = unsafe { WaitForSingleObject(handle as _, timeout.as_millis() as u32) };
    waited == WAIT_OBJECT_0
}

// windows-sys only exports this under `Storage::FileSystem`.
const SYNCHRONIZE: u32 = 0x0010_0000;

pub fn instance_taken(name: &str) -> bool {
    let scoped = wide(&format!(r"Local\{name}"));
    let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, scoped.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    unsafe { CloseHandle(handle) };
    true
}

pub fn tray_running() -> bool {
    instance_taken(TRAY)
}

pub fn signal_config_changed() {
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide(CONFIG_EVENT).as_ptr()) };
    if !handle.is_null() {
        unsafe {
            SetEvent(handle);
            CloseHandle(handle);
        }
    }
}

pub fn focus_window(title: &str) {
    let window = unsafe { FindWindowW(std::ptr::null(), wide(title).as_ptr()) };
    if !window.is_null() {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
}
