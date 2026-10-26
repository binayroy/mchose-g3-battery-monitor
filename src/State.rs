// SPDX-License-Identifier: GPL-3.0-or-later
//! State shared between the HID worker thread and the UI thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// A battery level at or below this (in %) triggers the low-battery alert.
pub const LOW_BATTERY_PERCENT: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    Starting,
    Checking,
    Connected,
    Disconnected,
    Error,
}

/// Everything the UI needs to draw the tray icon, tooltip and menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub percentage: Option<u8>,
    pub status: LinkStatus,
    /// Is the 2.4 GHz receiver (dongle) present?
    pub receiver: bool,
}

struct Inner {
    snapshot: Snapshot,
    last_live_event: Option<Instant>,
    low_alert_active: bool,
}

pub struct Shared {
    inner: Mutex<Inner>,
    stop: AtomicBool,
    refresh: AtomicBool,
    notifications: AtomicBool,
}

/// Decides whether a low-battery alert is due for this reading.
/// It fires once, then re-arms when the level goes back above the threshold.
fn low_alert_due(inner: &mut Inner, percentage: Option<u8>) -> bool {
    match percentage {
        Some(p) if p <= LOW_BATTERY_PERCENT => {
            let due = !inner.low_alert_active;
            inner.low_alert_active = true;
            due
        }
        Some(_) => {
            inner.low_alert_active = false;
            false
        }
        None => false,
    }
}

impl Shared {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    percentage: None,
                    status: LinkStatus::Starting,
                    receiver: true,
                },
                last_live_event: None,
                low_alert_active: false,
            }),
            stop: AtomicBool::new(false),
            refresh: AtomicBool::new(false),
            notifications: AtomicBool::new(true),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.lock().snapshot
    }

    // ---- state transitions used by the monitor thread ----

    /// Cached battery query succeeded (dongle present, mouse state still unknown).
    /// Returns true if a low-battery notification should be shown now.
    pub fn set_cached(&self, percentage: Option<u8>) -> bool {
        let mut g = self.lock();
        g.snapshot.percentage = percentage;
        g.snapshot.status = LinkStatus::Checking;
        g.snapshot.receiver = true;
        g.last_live_event = Some(Instant::now());

        let due = low_alert_due(&mut g, percentage);
        due && self.notifications_enabled()
    }

    /// Cached battery query failed (dongle missing / unreadable).
    pub fn set_unreachable(&self) {
        let mut g = self.lock();
        g.snapshot.status = LinkStatus::Disconnected;
        g.snapshot.receiver = false;
    }

    /// The HID subsystem itself could not be initialised.
    pub fn set_error(&self) {
        self.lock().snapshot.status = LinkStatus::Error;
    }

    pub fn mark_disconnected(&self) {
        self.lock().snapshot.status = LinkStatus::Disconnected;
    }

    pub fn begin_session(&self) {
        let mut g = self.lock();
        g.snapshot.status = LinkStatus::Checking;
        g.last_live_event = Some(Instant::now());
    }

    /// Live battery report. Returns true if a low-battery notification should be shown now.
    pub fn live_battery(&self, percentage: u8) -> bool {
        let mut g = self.lock();
        g.snapshot.percentage = Some(percentage);
        g.snapshot.status = LinkStatus::Connected;
        g.last_live_event = Some(Instant::now());

        let due = low_alert_due(&mut g, Some(percentage));
        due && self.notifications_enabled()
    }

    /// Live connection report from the dongle (mouse on/off).
    pub fn live_link(&self, connected: bool) {
        let mut g = self.lock();
        g.last_live_event = Some(Instant::now());
        g.snapshot.status = if connected {
            LinkStatus::Connected
        } else {
            LinkStatus::Disconnected
        };
    }

    /// If no live report arrived for `limit`, flip to Disconnected.
    /// Returns true when that flip just happened.
    pub fn check_timeout(&self, limit: Duration) -> bool {
        let mut g = self.lock();
        let timed_out = g
            .last_live_event
            .map(|t| t.elapsed() >= limit)
            .unwrap_or(false)
            && g.snapshot.status != LinkStatus::Disconnected;

        if timed_out {
            g.snapshot.status = LinkStatus::Disconnected;
        }
        timed_out
    }

    // ---- control flags ----

    pub fn notifications_enabled(&self) -> bool {
        self.notifications.load(Ordering::SeqCst)
    }

    pub fn set_notifications_enabled(&self, enabled: bool) {
        self.notifications.store(enabled, Ordering::SeqCst);
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn request_refresh(&self) {
        self.refresh.store(true, Ordering::SeqCst);
    }

    pub fn refresh_pending(&self) -> bool {
        self.refresh.load(Ordering::SeqCst)
    }

    pub fn clear_refresh(&self) {
        self.refresh.store(false, Ordering::SeqCst);
    }

    /// Sleep that wakes early on quit or "Refresh Now".
    pub fn sleep(&self, duration: Duration) {
        let end = Instant::now() + duration;
        while !self.should_stop() && !self.refresh_pending() {
            let now = Instant::now();
            if now >= end {
                break;
            }
            thread::sleep((end - now).min(Duration::from_millis(100)));
        }
    }
}