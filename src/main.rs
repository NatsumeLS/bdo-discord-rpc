#![windows_subsystem = "windows"]

mod config;
mod phase;
mod read;
mod show;
mod ui;
mod watch;
mod win;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::read::game::{self, GameFinder};
use crate::read::log_tail::{GameState, LogTail};
use crate::show::discord;
use crate::show::presence::{build, context};
use crate::ui::{prompt, settings, tray};
use crate::watch::{
    refresh_profile, resolve_family, resolve_game, resolve_region, resolve_user_data_dir, Shared,
};
use crate::win::log;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str);

    if matches!(mode, Some(arg) if arg != "--settings" && prompt::from_flag(arg).is_none()) {
        win::attach_console();
    }

    let exit = match mode {
        None => run(),
        Some("--settings") => settings::run(match args.get(1).map(String::as_str) {
            Some("log") => settings::Page::Log,
            _ => settings::Page::Overview,
        }),
        Some("--probe") => probe(),
        Some("--help" | "-h") => {
            print_help();
            0
        }
        Some(other) => match prompt::from_flag(other) {
            Some(table) => prompt::run(table, args.get(1).cloned()),
            None => {
                eprintln!("Unknown Argument: {other}");
                print_help();
                2
            }
        },
    };
    std::process::exit(exit);
}

fn print_help() {
    println!("bdo-discord-rpc {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("  bdo-discord-rpc                                  Run in the System Tray and update Discord");
    println!("  bdo-discord-rpc --settings                       Open the Settings Window");
    println!("  bdo-discord-rpc --probe                          Print what can be read right now, then exit");
    println!("  bdo-discord-rpc --name-server <server-key>       Ask for the Name of that Server");
    println!(
        "  bdo-discord-rpc --name-character <character-id>  Ask for the Name of that Character"
    );
    println!();
    println!("Config: {}", config::config_path().display());
}

fn run() -> i32 {
    // Before the log is opened, so a refused second tray cannot rotate the
    // running one's log away.
    if !win::claim_instance(win::TRAY) {
        win::attach_console();
        eprintln!("A Tray is already running");
        return 0;
    }
    win::enable_logfile(config::log_path());
    log(&format!("Started version {}", env!("CARGO_PKG_VERSION")));

    let shared = Arc::new(Shared::default());
    let worker = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || watch::watch(&shared))
    };

    let mut tray = match tray::Tray::new(win::startup_enabled()) {
        Ok(tray) => tray,
        Err(e) => {
            win::error(&format!("Tray: {e}"));
            shared.quit.store(true, Ordering::Relaxed);
            let _ = worker.join();
            return 1;
        }
    };

    let mut shown = watch::Status::default();
    let mut settings = win::ChildWindow::default();
    let mut config_error_shown = false;
    loop {
        match tray.pump() {
            Some(tray::Action::Quit) => break,
            Some(tray::Action::ToggleSettings) => {
                if settings.is_open() {
                    settings.close();
                } else {
                    open_settings(&mut settings, &["--settings"]);
                }
            }
            Some(tray::Action::ToggleStartup(wanted)) => match win::set_startup(wanted) {
                Ok(()) => log(&format!(
                    "Run at Startup: {}",
                    if wanted { "Enabled" } else { "Disabled" }
                )),
                Err(e) => {
                    win::error(&format!("Run at Startup: {e}"));
                    tray.set_startup_checked(win::startup_enabled());
                }
            },
            Some(tray::Action::ReloadConfig) => {
                log("Config: Reload asked from the Tray");
                win::signal_config_changed();
            }
            None => {}
        }

        let latest = shared.status.lock().ok().map(|status| status.clone());
        if let Some(latest) = latest {
            if latest != shown {
                shown = latest;
                tray.set_status(shown.health, &shown.line);
            }
        }

        if !config_error_shown && shared.config_error.load(Ordering::Relaxed) {
            config_error_shown = true;
            open_settings(&mut settings, &["--settings", "log"]);
        }

        if worker.is_finished() {
            break;
        }

        std::thread::sleep(Duration::from_millis(150));
    }

    settings.close();

    shared.quit.store(true, Ordering::Relaxed);
    worker.join().unwrap_or(1)
}

fn open_settings(settings: &mut win::ChildWindow, args: &[&str]) {
    if let Err(e) = settings.open(args) {
        win::error(&format!("Could not open the Settings Window: {e}"));
    }
}

fn probe() -> i32 {
    let path = config::config_path();
    let config = match config::load_or_create(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Config Error: {e}");
            return 1;
        }
    };
    log(&format!("Config: {}", path.display()));

    let mut finder = GameFinder::default();
    let mut state = GameState::default();
    let mut region = String::new();

    match resolve_game(&config, &mut finder) {
        Some(process) => {
            let root = &process.root;
            region = resolve_region(&config, root).unwrap_or_default();
            println!("Game Root : {}", root.display());
            println!("Region    : {}", or_unknown(&region));

            let mut tail = LogTail::new(root, process.started_at);
            tail.poll(&mut state);

            println!(
                "Log File  : {}",
                tail.file_name().unwrap_or_else(|| "(none)".into())
            );
            println!();
            println!("Phase         : {}", phase::label(state.phase));
            println!("Phase since   : {}", format_stamp(state.phase_since));
            println!("Session Start : {}", format_stamp(state.session_start));
            println!(
                "Game Server   : {}",
                state.game_server.as_deref().unwrap_or("(none)")
            );
        }
        None => println!("Game Root : (not running, and no Override in the Config)"),
    }

    let user_data = resolve_user_data_dir(&config);
    let family = resolve_family(&config);
    let character = watch::play_started_at(&state)
        .zip(user_data.as_deref())
        .and_then(|(started, dir)| game::detect_character(dir, started));
    match &user_data {
        Some(dir) => {
            println!("User Data : {}", dir.display());
            println!(
                "Family    : {}",
                or_unknown(family.as_deref().unwrap_or_default())
            );
            println!(
                "Character : {} ({})",
                or_unknown(character.as_deref().unwrap_or_default()),
                character
                    .as_deref()
                    .and_then(|key| config.characters.get(key))
                    .map(String::as_str)
                    .unwrap_or("unnamed")
            );
            ui::m3::describe(&config);
        }
        None => println!("User Data : (could not resolve the Documents Folder)"),
    }

    let mut profile = None;
    refresh_profile(&config, family.as_deref(), &mut None, &mut profile);
    let ctx = context(
        &config,
        &state,
        family.as_deref(),
        &region,
        character.as_deref(),
        profile.as_ref(),
    );

    println!();
    println!("Presence Payload Discord receives:");
    match build(&config, &state, &ctx) {
        Some(fields) => println!("{}", discord::preview(&fields)),
        None => println!("  (this Phase is not reported)"),
    }

    0
}

fn or_unknown(value: &str) -> &str {
    if value.is_empty() {
        "(unknown)"
    } else {
        value
    }
}

fn format_stamp(unix: Option<i64>) -> String {
    watch::stamp(unix).unwrap_or_else(|| "-".into())
}
