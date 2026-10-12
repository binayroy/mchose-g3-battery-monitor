// Hides the console terminal window when launching on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use auto_launch::AutoLaunch;
use hidapi::HidApi;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};
use winit::event_loop::{ControlFlow, EventLoop};

// MCHOSE G3 USB HID Identifiers
const VENDOR_ID: u16 = 0x258A;
const PRODUCT_ID: u16 = 0x0138;
const REPORT_ID: u8 = 0x09;
const REPORT_SIZE: usize = 520;

const APP_NAME: &str = "MCHOSE G3 Battery Monitor";
const APP_VERSION: &str = "v0.9.1";

#[derive(Clone, Debug)]
enum BatteryStatus {
    Connected(u8),
    Disconnected,
}

fn main() {
    // 1. Configure Auto-Launch / Startup settings
    let exe_path = std::env::current_exe().unwrap_or_default();
    let auto_launcher = AutoLaunch::new(
        "MCHOSE_G3_Battery_Monitor",
        exe_path.to_str().unwrap_or(""),
        &["--minimized"],
    );

    // 2. Check startup status at launch; if not enabled, ask user with a Yes/No dialog
    let mut is_autostart_active = auto_launcher.is_enabled().unwrap_or(false);
    if !is_autostart_active {
        let user_choice = rfd::MessageDialog::new()
            .set_title(APP_NAME)
            .set_description("Would you like MCHOSE G3 Battery Monitor to launch automatically when your computer starts up?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();

        if user_choice == rfd::MessageDialogResult::Yes {
            if auto_launcher.enable().is_ok() {
                is_autostart_active = true;
            }
        }
    }

    // 3. Initialize Event Loop (Required for Windows tray events & right-click menus)
    let event_loop = EventLoop::new().unwrap();

    // 4. Build System Tray Right-Click Menu
    let tray_menu = Menu::new();
    
    let name_item = MenuItem::new(APP_NAME, false, None);
    let version_item = MenuItem::new(format!("Version: {}", APP_VERSION), false, None);
    let status_menu_item = MenuItem::new("Status: Connecting...", false, None);
    let refresh_menu_item = MenuItem::new("Refresh Now", true, None);

    let autostart_menu_item = CheckMenuItem::new(
        "Launch at Startup",
        true,
        is_autostart_active,
        None,
    );

    let quit_menu_item = MenuItem::new("Quit", true, None);

    tray_menu.append(&name_item).unwrap();
    tray_menu.append(&version_item).unwrap();
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();
    tray_menu.append(&status_menu_item).unwrap();
    tray_menu.append(&refresh_menu_item).unwrap();
    tray_menu.append(&autostart_menu_item).unwrap();
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();
    tray_menu.append(&quit_menu_item).unwrap();

    let quit_id = quit_menu_item.id().clone();
    let refresh_id = refresh_menu_item.id().clone();
    let autostart_id = autostart_menu_item.id().clone();

    // 5. Initial Tray Icon (Start with 0% / Disconnected battery graphic)
    let initial_icon = generate_battery_icon(None);
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("MCHOSE G3: Initializing...")
        .with_icon(initial_icon)
        .build()
        .expect("Failed to initialize system tray icon");

    // 6. Thread-safe status storage
    let current_status = Arc::new(Mutex::new(BatteryStatus::Disconnected));
    let status_clone = Arc::clone(&current_status);

    // 7. Spawn background polling thread
    let hid_api_thread = HidApi::new().ok();
    thread::spawn(move || {
        let Some(ref api) = hid_api_thread else { return; };
        loop {
            let battery = poll_mouse_battery(api);
            {
                let mut status = status_clone.lock().unwrap();
                *status = battery;
            }
            thread::sleep(Duration::from_secs(30));
        }
    });

    // 8. Main Event Loop using Winit (Optimized cadence to prevent UI jitter)
    let menu_channel = MenuEvent::receiver();
    let tray_icon_ref = Arc::new(tray_icon);
    let tray_icon_loop = Arc::clone(&tray_icon_ref);

    let _ = event_loop.run(move |_event, elwt| {
        // Calm wakeup cadence (1 second) prevents CPU throttling and right-click jitter
        elwt.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_secs(1)
        ));

        // Handle menu interactions
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_id {
                elwt.exit();
            } else if event.id == refresh_id {
                if let Ok(hid_api) = HidApi::new() {
                    let battery = poll_mouse_battery(&hid_api);
                    let mut status = current_status.lock().unwrap();
                    *status = battery.clone();
                }
            } else if event.id == autostart_id {
                let is_checked = autostart_menu_item.is_checked();
                if is_checked {
                    let _ = auto_launcher.enable();
                } else {
                    let _ = auto_launcher.disable();
                }
            }
        }

        let status = {
            let s = current_status.lock().unwrap();
            s.clone()
        };

        update_tray_ui(&tray_icon_loop, &status_menu_item, &status);
    });
}

/// Dynamically generates a clean 32x32 battery icon with fill percentage
fn generate_battery_icon(percentage: Option<u8>) -> Icon {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; width * height * 4];

    let pct = percentage.unwrap_or(0);

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            
            let is_border = (x == 4 || x == 25) && (y >= 8 && y <= 24)
                || (y == 8 || y == 24) && (x >= 4 && x <= 25);
            
            let is_terminal = (x == 26 || x == 27) && (y >= 14 && y <= 18);

            let inner_width = 18;
            let fill_width = ((inner_width as f32) * (pct as f32 / 100.0)) as usize;
            let is_fill = percentage.is_some() 
                && (x >= 6 && x < 6 + fill_width) 
                && (y >= 10 && y <= 22);

            if is_border || is_terminal {
                rgba[idx] = 220;     // R
                rgba[idx + 1] = 220; // G
                rgba[idx + 2] = 220; // B
                rgba[idx + 3] = 255; // A
            } else if is_fill {
                if pct < 20 {
                    rgba[idx] = 230; rgba[idx + 1] = 50; rgba[idx + 2] = 50;
                } else if pct < 50 {
                    rgba[idx] = 240; rgba[idx + 1] = 200; rgba[idx + 2] = 50;
                } else {
                    rgba[idx] = 50; rgba[idx + 1] = 205; rgba[idx + 2] = 50;
                }
                rgba[idx + 3] = 255;
            } else {
                rgba[idx + 3] = 0;
            }
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).unwrap()
}

/// Scans all device interfaces matching the mouse to find the correct feature report channel

fn poll_mouse_battery(api: &HidApi) -> BatteryStatus {
    const USAGE_PAGE: u16 = 0xFF02;
    const USAGE: u16 = 0x0002;

    const QUERY_PREFIX: [u8; 8] =
        [0x09, 0x0C, 0x09, 0x01, 0x01, 0x00, 0x01, 0x00];

    // Look specifically for the HID collection used by the working Python script.
    let paths: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == VENDOR_ID
                && d.product_id() == PRODUCT_ID
                && d.usage_page() == USAGE_PAGE
                && d.usage() == USAGE
        })
        .map(|d| d.path().to_owned())
        .collect();

    for path in paths {
        // Retry briefly in case the mouse has just woken up.
        for attempt in 0..3 {
            if let Ok(device) = api.open_path(&path) {
                let mut query = vec![0u8; REPORT_SIZE];
                query[..QUERY_PREFIX.len()].copy_from_slice(&QUERY_PREFIX);

                // Match the Python script: send the query before reading.
                if device.send_feature_report(&query).is_ok() {
                    thread::sleep(Duration::from_millis(50));

                    let mut response = vec![0u8; REPORT_SIZE];
                    response[0] = REPORT_ID;

                    if let Ok(length) =
                        device.get_feature_report(&mut response)
                    {
                        if length >= 9
                            && response[..8] == QUERY_PREFIX
                            && response[8] <= 100
                        {
                            return BatteryStatus::Connected(response[8]);
                        }
                    }
                }
            }

            if attempt < 2 {
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    BatteryStatus::Disconnected
}


/// Updates tray tooltip, menu text, and dynamic battery icon
fn update_tray_ui(
    tray: &tray_icon::TrayIcon,
    status_item: &MenuItem,
    status: &BatteryStatus,
) {
    match status {
        BatteryStatus::Connected(pct) => {
            let _ = tray.set_tooltip(Some(format!("MCHOSE G3: {}%", pct)));
            let _ = tray.set_icon(Some(generate_battery_icon(Some(*pct))));
            status_item.set_text(format!("Status: Connected ({}%)", pct));
        }
        BatteryStatus::Disconnected => {
            let _ = tray.set_tooltip(Some("MCHOSE G3: Disconnected / Sleeping"));
            let _ = tray.set_icon(Some(generate_battery_icon(None)));
            status_item.set_text("Status: Disconnected / Sleeping");
        }
    }
}