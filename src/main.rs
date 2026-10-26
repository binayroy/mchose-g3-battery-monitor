// SPDX-License-Identifier: GPL-3.0-or-later
// Hides the console window when launching on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
compile_error!("This app supports Windows and Linux only.");

#[path = "Autostart.rs"]
mod autostart;

mod debuglog;

#[path = "Icon.rs"]
mod icon;

#[path = "Monitor.rs"]
mod monitor;

#[path = "State.rs"]
mod state;

use std::sync::Arc;
use std::thread;
use std::time::{Duration as StdDuration, Instant as StdInstant};
 
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder,
};
 
use icon::generate_battery_icon;
use state::{LinkStatus, Shared, Snapshot};
 
const APP_NAME: &str = "MCHOSE G3 Battery Monitor";
const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
 
// ======================================================================
// Text helpers (wording taken from the Python app)
// ======================================================================
 
fn status_text(s: &Snapshot) -> String {
    match s.status {
        LinkStatus::Connected => match s.percentage {
            Some(p) => format!("Connected - Battery: {p}%"),
            None => "Connected - Battery unknown".to_string(),
        },
        LinkStatus::Disconnected => "Mouse: Disconnected / Sleeping".to_string(),
        LinkStatus::Checking => match s.percentage {
            Some(p) => format!(" - Battery: {p}%"),
            None => "Checking Live Battery".to_string(),
        },
        LinkStatus::Error => "Monitor error".to_string(),
        LinkStatus::Starting => "Starting...".to_string(),
    }
}
 
fn receiver_text(s: &Snapshot) -> &'static str {
    if s.receiver {
        "Receiver: Connected"
    } else {
        "Receiver: Disconnected"
    }
}
 
fn tooltip_text(s: &Snapshot) -> String {
    match (s.status, s.percentage) {
        (LinkStatus::Connected | LinkStatus::Checking, Some(p)) => format!("MCHOSE G3: {p}%"),
        (LinkStatus::Error, _) => "MCHOSE G3: Monitor error".to_string(),
        (LinkStatus::Starting, _) => "MCHOSE G3: Starting".to_string(),
        (_, Some(p)) => format!("MCHOSE G3: Disconnected / Sleeping - last battery {p}%"),
        (_, None) => "MCHOSE G3: Disconnected / Sleeping".to_string(),
    }
}
 
// ======================================================================
// Startup prompt (asked once, then remembered)
// ======================================================================
 
const AUTOSTART_QUESTION: &str =
    "Would you like MCHOSE G3 Battery Monitor to launch automatically when you sign in?";
 
#[cfg(target_os = "windows")]
fn ask_autostart() -> bool {
    rfd::MessageDialog::new()
        .set_title(APP_NAME)
        .set_description(AUTOSTART_QUESTION)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}
 
/// Native GTK dialog (GTK is already initialised by run()).
#[cfg(target_os = "linux")]
fn ask_autostart() -> bool {
    use gtk::prelude::*;
 
    let dialog = gtk::MessageDialog::builder()
        .message_type(gtk::MessageType::Question)
        .buttons(gtk::ButtonsType::YesNo)
        .title(APP_NAME)
        .text(AUTOSTART_QUESTION)
        .build();
 
    let answer = dialog.run();
    dialog.close();
    // Let GTK actually remove the dialog from the screen.
    while gtk::events_pending() {
        gtk::main_iteration();
    }
 
    answer == gtk::ResponseType::Yes
}
 
/// Returns whether autostart is active after the (possible) prompt.
fn startup_prompt() -> bool {
    autostart::sync_path();
    let mut active = autostart::is_enabled();
 
    if !autostart::prompt_shown() {
        if !active {
            crate::dlog!("[MAIN] showing startup prompt");
            let wants_autostart = ask_autostart();
            crate::dlog!("[MAIN] startup prompt answered: {wants_autostart}");
 
            if wants_autostart && autostart::set_enabled(true).is_ok() {
                active = true;
            }
        }
        // Remember that we asked, whatever the answer was.
        autostart::mark_prompt_shown();
    }
 
    active
}
 
// ======================================================================
// Tray application
// ======================================================================
 
struct App {
    tray: TrayIcon,
    status_item: MenuItem,
    receiver_item: MenuItem,
    autostart_item: CheckMenuItem,
    notifications_item: CheckMenuItem,
    quit_id: MenuId,
    refresh_id: MenuId,
    autostart_id: MenuId,
    notifications_id: MenuId,
    shared: Arc<Shared>,
    last_snapshot: Option<Snapshot>,
    last_icon: Option<Option<u8>>,
    last_beat: StdInstant,
}
 
impl App {
    /// Must be called after the platform UI loop is initialised
    /// (Win32 message loop on Windows, gtk::init on Linux).
    fn new(shared: Arc<Shared>) -> Self {
        crate::dlog!("[MAIN] building tray application");
        let autostart_active = startup_prompt();
 
        let menu = Menu::new();
        let name_item = MenuItem::new(APP_NAME, false, None);
        let version_item = MenuItem::new(format!("Version: {APP_VERSION}"), false, None);
        let status_item = MenuItem::new("Status: Starting...", true, None);
        let receiver_item = MenuItem::new("Receiver: Connected", false, None);
        let refresh_item = MenuItem::new("Refresh Now", true, None);
        let autostart_item = CheckMenuItem::new("Launch at Startup", true, autostart_active, None);
        let notifications_item = CheckMenuItem::new(
            "Low Battery Notification",
            true,
            shared.notifications_enabled(),
            None,
        );
        let quit_item = MenuItem::new("Quit", true, None);
 
        menu.append(&name_item).unwrap();
        menu.append(&version_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&status_item).unwrap();
        menu.append(&receiver_item).unwrap();
        menu.append(&refresh_item).unwrap();
        menu.append(&autostart_item).unwrap();
        menu.append(&notifications_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&quit_item).unwrap();
 
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("MCHOSE G3: Starting")
            .with_icon(generate_battery_icon(None))
            .build()
            .expect("Failed to initialize system tray icon");
        crate::dlog!("[MAIN] tray icon created");
 
        App {
            tray,
            quit_id: quit_item.id().clone(),
            refresh_id: refresh_item.id().clone(),
            autostart_id: autostart_item.id().clone(),
            notifications_id: notifications_item.id().clone(),
            status_item,
            receiver_item,
            autostart_item,
            notifications_item,
            shared,
            last_snapshot: None,
            last_icon: Some(None),
            last_beat: StdInstant::now(),
        }
    }
 
    /// Called periodically by the platform loop. Returns true when the app should quit.
    fn tick(&mut self) -> bool {
        // Heartbeat: shows in the debug log whether the UI thread is alive.
        if self.last_beat.elapsed() >= StdDuration::from_secs(1) {
            self.last_beat = StdInstant::now();
            //crate::dlog!("[UI] tick alive");
        }
 
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit_id {
                self.shared.request_stop();
                return true;
            } else if event.id == self.refresh_id {
                self.shared.request_refresh();
            } else if event.id == self.autostart_id {
                self.toggle_autostart();
            } else if event.id == self.notifications_id {
                // The check item flips itself on click; apply and remember the new state.
                let enabled = self.notifications_item.is_checked();
                self.shared.set_notifications_enabled(enabled);
                autostart::set_notifications_enabled(enabled);
            }
        }
 
        self.render();
        false
    }
 
    fn toggle_autostart(&self) {
        // The check item flips itself on click; apply the new state.
        let wanted = self.autostart_item.is_checked();
        if let Err(e) = autostart::set_enabled(wanted) {
            crate::dlog!("[WARN] Could not change autostart: {e}");
        }
        // Show what actually happened.
        self.autostart_item.set_checked(autostart::is_enabled());
    }
 
    /// Only touches the tray when something actually changed.
    fn render(&mut self) {
        let snap = self.shared.snapshot();
        if self.last_snapshot == Some(snap) {
            return;
        }
        self.last_snapshot = Some(snap);
 
        // Battery graphic only while the mouse is believed to be reachable;
        // otherwise show the warning icon (same as the Python app).
        let icon_pct = match snap.status {
            LinkStatus::Connected | LinkStatus::Checking => snap.percentage,
            _ => None,
        };
        if self.last_icon != Some(icon_pct) {
            let _ = self.tray.set_icon(Some(generate_battery_icon(icon_pct)));
            self.last_icon = Some(icon_pct);
        }
 
        let _ = self.tray.set_tooltip(Some(tooltip_text(&snap)));
        self.status_item.set_text(status_text(&snap));
        self.receiver_item.set_text(receiver_text(&snap));
    }
}
 
// ======================================================================
// Platform event loops
// ======================================================================
 
#[cfg(target_os = "windows")]
fn run(shared: Arc<Shared>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG,
        PM_REMOVE, QS_ALLINPUT, WM_QUIT,
    };
 
    // Plain Win32 message loop (what tray-icon expects on Windows).
    // The tray icon and its menu create their own hidden window on this
    // thread, so the thread only has to keep pumping messages.
    let mut app = App::new(shared);
 
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        'main: loop {
            // Handle every pending message first.
            while PeekMessageW(&mut msg, std::mem::zeroed(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    break 'main;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
 
            if app.tick() {
                break;
            }
 
            // Sleep until a message arrives or 250 ms pass.
            MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 250, QS_ALLINPUT);
        }
    }
}
 
#[cfg(target_os = "linux")]
fn run(shared: Arc<Shared>) {
    use gtk::glib::ControlFlow;
    use std::time::Duration;
 
    // The tray (libappindicator) and menus need a running GTK main loop.
    gtk::init().expect("Failed to initialise GTK (is a display available?)");
    crate::dlog!("[MAIN] GTK initialised");
    let mut app = App::new(shared);
 
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        if app.tick() {
            gtk::main_quit();
            ControlFlow::Break
        } else {
            ControlFlow::Continue
        }
    });
 
    crate::dlog!("[MAIN] entering GTK main loop");
    gtk::main();
}
 
fn main() {
    debuglog::init();
    crate::dlog!("[MAIN] started");
 
    // Linux: make sure the app has a launcher entry (with icon) in the app menu.
    autostart::install_launcher();
 
    let shared = Arc::new(Shared::new());
    // Saved preference (default: on).
    shared.set_notifications_enabled(autostart::notifications_enabled());
 
    // Background HID worker (cached query, then live reports).
    let worker_state = Arc::clone(&shared);
    thread::spawn(move || monitor::run(worker_state));
 
    run(shared);
}