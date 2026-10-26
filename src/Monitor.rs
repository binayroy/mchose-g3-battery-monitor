// SPDX-License-Identifier: GPL-3.0-or-later
//! HID worker: cached battery query, then live report monitoring.
//! Direct port of the working Python script.

use std::ffi::{CStr, CString};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hidapi::HidApi;

use crate::state::Shared;

// MCHOSE G3 USB HID identifiers
const VENDOR_ID: u16 = 0x258A;
const PRODUCT_ID: u16 = 0x0138;
const USAGE_PAGE: u16 = 0xFF02;
const USAGE: u16 = 0x0002;

const REPORT_ID: u8 = 0x09;
const REPORT_SIZE: usize = 520;

const LIVE_TIMEOUT: Duration = Duration::from_secs(55);
const CACHE_RETRY: Duration = Duration::from_secs(2);
const READ_TIMEOUT_MS: i32 = 1000;

/// Live battery report:    09 0A 05 <percentage>
const BATTERY_PREFIX: [u8; 3] = [0x09, 0x0A, 0x05];
/// Live connection report: 09 0A 02 <00 = off/sleeping, 01 = connected>
const STATUS_PREFIX: [u8; 3] = [0x09, 0x0A, 0x02];

/// Legacy "cached battery" feature-report query.
const QUERY_PREFIX: [u8; 8] = [0x09, 0x0C, 0x09, 0x01, 0x01, 0x00, 0x01, 0x00];

enum SessionEnd {
    Stop,
    Refresh,
    HidError,
}

pub fn run(shared: Arc<Shared>) {
    // Create the HID context (retry if it fails).
    let mut api = loop {
        if shared.should_stop() {
            return;
        }
        match HidApi::new() {
            Ok(api) => break api,
            Err(e) => {
                crate::dlog!("[ERROR] HID init failed: {e}");
                shared.set_error();
                shared.sleep(CACHE_RETRY);
            }
        }
    };

    while !shared.should_stop() {
        // Phase 1: cached battery. Retry every 2 s until the dongle answers.
        let path = loop {
            if shared.should_stop() {
                return;
            }
            shared.clear_refresh();

            match cached_battery(&mut api) {
                Ok((percentage, path)) => {
                    crate::dlog!("[CACHE] Request successful: {percentage:?}");
                    // Low battery at startup (or after a dongle re-plug) -> notify once.
                    if shared.set_cached(percentage) {
                        if let Some(p) = percentage {
                            notify_low_battery(p);
                        }
                    }
                    break path;
                }
                Err(e) => {
                    crate::dlog!("[CACHE] Request failed: {e}");
                    shared.set_unreachable();
                    shared.sleep(CACHE_RETRY);
                }
            }
        };

        // Phase 2: live reports. On failure, go back to phase 1.
        match monitor_session(&api, &path, &shared) {
            SessionEnd::Stop => break,
            SessionEnd::Refresh => {}
            SessionEnd::HidError => {
                crate::dlog!("[RECOVERY] Live monitor failed. Returning to cache recovery.");
                shared.mark_disconnected();
                shared.sleep(CACHE_RETRY);
            }
        }
    }
}

/// HID collections that may carry the battery protocol.
fn candidate_paths(api: &HidApi) -> Vec<CString> {
    let dongle: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        .collect();

    // Windows exposes every top-level collection separately, so match the exact one.
    #[allow(unused_mut)]
    let mut paths: Vec<CString> = dongle
        .iter()
        .filter(|d| d.usage_page() == USAGE_PAGE && d.usage() == USAGE)
        .map(|d| d.path().to_owned())
        .collect();

    // On Linux (hidraw) usage info may only describe the first collection of an
    // interface, so also try the other interface-1 nodes. The response prefix is
    // validated afterwards, so a wrong node is harmless.
    #[cfg(not(target_os = "windows"))]
    for d in &dongle {
        if d.interface_number() == 1 {
            let path = d.path().to_owned();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    paths
}

/// Returns the cached percentage (0 % is treated as unavailable) and the HID
/// path that answered.
fn cached_battery(api: &mut HidApi) -> Result<(Option<u8>, CString), String> {
    // Pick up dongle plug/unplug since the last attempt.
    crate::dlog!("[HID] refresh_devices begin");
    api.refresh_devices()
        .map_err(|e| format!("device refresh failed: {e}"))?;
    crate::dlog!("[HID] refresh_devices done");

    let paths = candidate_paths(api);
    if paths.is_empty() {
        return Err("dongle HID collection not found".into());
    }

    let mut last_error = String::new();
    for path in paths {
        match query_cached(api, &path) {
            Ok(percentage) => return Ok((percentage, path)),
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

fn query_cached(api: &HidApi, path: &CStr) -> Result<Option<u8>, String> {
    crate::dlog!("[HID] query: open_path begin");
    let device = api
        .open_path(path)
        .map_err(|e| format!("open failed: {e}"))?;
    crate::dlog!("[HID] query: open_path ok");

    let mut query = vec![0u8; REPORT_SIZE];
    query[..QUERY_PREFIX.len()].copy_from_slice(&QUERY_PREFIX);

    device
        .send_feature_report(&query)
        .map_err(|e| format!("feature report send failed: {e}"))?;
    crate::dlog!("[HID] query: send_feature_report ok");

    thread::sleep(Duration::from_millis(50));

    let mut response = vec![0u8; REPORT_SIZE];
    response[0] = REPORT_ID;

    let length = device
        .get_feature_report(&mut response)
        .map_err(|e| format!("feature report read failed: {e}"))?;
    crate::dlog!("[HID] query: get_feature_report ok ({length} bytes)");

    if length < 9 || response[..8] != QUERY_PREFIX {
        return Err(format!(
            "unexpected cached battery response: {:02X?}",
            &response[..length.min(16)]
        ));
    }

    let percentage = response[8];
    if percentage > 100 {
        return Err(format!("invalid cached battery value: {percentage}"));
    }

    Ok(if percentage == 0 { None } else { Some(percentage) })
}

fn monitor_session(api: &HidApi, path: &CStr, shared: &Shared) -> SessionEnd {
    let device = match api.open_path(path) {
        Ok(d) => d,
        Err(e) => {
            crate::dlog!("[ERROR] Cannot open live monitor: {e}");
            return SessionEnd::HidError;
        }
    };

    shared.begin_session();
    crate::dlog!("[DEBUG] Live monitor started; timeout is {LIVE_TIMEOUT:?}.");

    let end = monitor_loop(&device, shared);

    crate::dlog!("[HID] session over, closing device");
    drop(device);
    crate::dlog!("[HID] device closed");
    end
}

fn monitor_loop(device: &hidapi::HidDevice, shared: &Shared) -> SessionEnd {
    let mut buf = [0u8; 64];
    loop {
        if shared.should_stop() {
            return SessionEnd::Stop;
        }
        if shared.refresh_pending() {
            return SessionEnd::Refresh;
        }

        match device.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => {}
            Ok(n) => handle_live_report(&buf[..n], shared),
            Err(e) => {
                crate::dlog!("[ERROR] HID read error: {e}");
                return SessionEnd::HidError;
            }
        }

        if shared.check_timeout(LIVE_TIMEOUT) {
            crate::dlog!("[TIMEOUT] No live report for {LIVE_TIMEOUT:?}. Disconnected / Sleeping.");
        }
    }
}

fn handle_live_report(report: &[u8], shared: &Shared) {
    if report.len() < 4 {
        return;
    }

    if report[..3] == BATTERY_PREFIX {
        let percentage = report[3];
        if percentage > 100 {
            crate::dlog!("[WARN] Invalid live battery value: {percentage}");
            return;
        }
        crate::dlog!("[LIVE] Battery: {percentage}%");
        if shared.live_battery(percentage) {
            notify_low_battery(percentage);
        }
    } else if report[..3] == STATUS_PREFIX {
        match report[3] {
            0x00 => {
                crate::dlog!("[STATUS] Disconnected / Sleeping");
                shared.live_link(false);
            }
            0x01 => {
                crate::dlog!("[STATUS] Connected / Active");
                shared.live_link(true);
            }
            other => crate::dlog!("[INFO] Unknown status byte: 0x{other:02X}"),
        }
    }
}

/// Desktop notification, sent from a short-lived thread so HID reads never block.
fn notify_low_battery(percentage: u8) {
    thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("MCHOSE G3 Battery Monitor")
            .summary("MCHOSE G3: Low battery")
            .body(&format!("Mouse battery is {percentage}%. Charge it soon."))
            .show();
    });
}