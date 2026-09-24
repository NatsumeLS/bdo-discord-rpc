use serde::{Deserialize, Serialize};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

pub const TOOLTIP: &str = "Black Desert Discord Rich Presence";

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Health {
    Idle,
    Waiting,
    Live,
}

impl Health {
    fn tint(self) -> [u8; 3] {
        match self {
            Health::Idle => [0x6B, 0x72, 0x80],
            Health::Waiting => [0xD9, 0x8C, 0x2B],
            Health::Live => [0x3B, 0xA5, 0x5D],
        }
    }
}

pub enum Action {
    ToggleSettings,
    ToggleStartup(bool),
    ReloadConfig,
    Quit,
}

const ICON_SIZE: u32 = 32;

fn icon_for(health: Health) -> Option<Icon> {
    Icon::from_rgba(icon_rgba(health.tint()), ICON_SIZE, ICON_SIZE).ok()
}

pub fn icon_rgba(tint: [u8; 3]) -> Vec<u8> {
    let size = ICON_SIZE as f32;
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);

    let radius = 7.0_f32;
    let inset = 2.0_f32;
    let dot_center = (size * 0.62, size * 0.38);
    let dot_radius = size * 0.16;

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);

            let dx = (inset + radius - fx)
                .max(fx - (size - inset - radius))
                .max(0.0);
            let dy = (inset + radius - fy)
                .max(fy - (size - inset - radius))
                .max(0.0);
            let body = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);

            let ddx = fx - dot_center.0;
            let ddy = fy - dot_center.1;
            let dot = (dot_radius + 0.5 - (ddx * ddx + ddy * ddy).sqrt()).clamp(0.0, 1.0);

            for channel in tint {
                let lighter = channel as f32 + (255.0 - channel as f32) * 0.65;
                rgba.push((channel as f32 * (1.0 - dot) + lighter * dot) as u8);
            }
            rgba.push((body * 255.0) as u8);
        }
    }

    rgba
}

pub fn window_icon() -> Option<iced::window::Icon> {
    let seed = crate::ui::m3::FALLBACK_SEED;
    let tint = [(seed >> 16) as u8, (seed >> 8) as u8, seed as u8];
    iced::window::icon::from_rgba(icon_rgba(tint), ICON_SIZE, ICON_SIZE).ok()
}

pub struct Tray {
    icon: TrayIcon,
    health: Health,
    startup_item: CheckMenuItem,
    startup_id: MenuId,
    reload_id: MenuId,
    quit_id: MenuId,
}

impl Tray {
    pub fn new(startup_enabled: bool) -> Result<Tray, String> {
        let startup = CheckMenuItem::new("Run at Startup", true, startup_enabled, None);
        let reload = MenuItem::new("Reload Config", true, None);
        let quit = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[&startup, &reload, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| format!("Could not build the Tray Menu: {e}"))?;

        let health = Health::Idle;
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip(TOOLTIP);
        if let Some(icon) = icon_for(health) {
            builder = builder.with_icon(icon);
        }

        let icon = builder
            .build()
            .map_err(|e| format!("Could not create the Tray Icon: {e}"))?;

        Ok(Tray {
            icon,
            health,
            startup_id: startup.id().clone(),
            reload_id: reload.id().clone(),
            quit_id: quit.id().clone(),
            startup_item: startup,
        })
    }

    pub fn set_status(&mut self, health: Health, line: &str) {
        let _ = self.icon.set_tooltip(Some(line));

        if health != self.health {
            self.health = health;
            if let Some(icon) = icon_for(health) {
                let _ = self.icon.set_icon(Some(icon));
            }
        }
    }

    pub fn set_startup_checked(&self, checked: bool) {
        self.startup_item.set_checked(checked);
    }

    pub fn pump(&self) -> Option<Action> {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let action = if event.id == self.startup_id {
                Action::ToggleStartup(self.startup_item.is_checked())
            } else if event.id == self.reload_id {
                Action::ReloadConfig
            } else if event.id == self.quit_id {
                Action::Quit
            } else {
                continue;
            };
            return Some(action);
        }

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                return Some(Action::ToggleSettings);
            }
        }
        None
    }
}
