# MCHOSE G3 Battery Monitor

A small system-tray app that shows the battery level and connection state of the **MCHOSE G3** wireless mouse, written in Rust for **Windows** and **Linux**.

It reads the battery % straight from the 2.4 GHz USB receiver (dongle) over HID, and it knows when the mouse is switched off or asleep.

> Unofficial project. Not affiliated with or endorsed by MCHOSE.

## Supported platforms

| Platform | Notes |
|---|---|
| Windows 10 / 11 | Works out of the box. |
| Linux | Needs a tray that supports AppIndicator / StatusNotifier icons and a udev rule (see below). Tested on KDE Plasma, XFCE, GNOME, Cinnamon, MATE and Cosmic.|

> macOS is not supported.

# Download & Installation

## Windows

1. [**Download**](https://github.com/binayroy/mchose-g3-battery-monitor/releases/download/v1.0.0/mchose-g3-battery-monitor-windows-v1.0.0-amd64.exe) or take the `.exe` from `release`.
2. Run `mchose-g3-battery-monitor-windows-v*.*.*-amd64.exe`. Select autostart option and look for the icon in the tray (it may be inside the hidden-icons arrow).

No driver or extra setup is needed.

## Linux

**1. Install runtime libraries**

- Install `libgtk-3-0` and `libayatana-appindicator3-1`.
- If you're using Fedora: `libgtk-3-0` and `libayatana-appindicator-gtk3`.

2. [**Download**](https://github.com/binayroy/mchose-g3-battery-monitor/releases/download/v1.0.0/mchose-g3-battery-monitor-linux-v1.0.0-amd64-ELF) or take the ELF binary from `release` (Works with all the destros)

**3. Allow your current user account to read the dongle.** 
- Run the following bash commands on your terminal:
```bash
echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="258a", ATTRS{idProduct}=="0138", MODE="0660", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/70-mchose-g3.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
```
> It creates a udev rule. The file name must start with a low number such as `70-`: the `uaccess` tag that grants access to your logged-in session is ignored in files that sort after `73-seat-late.rules`.

> Reloads and triggers the new rules. 

- Might need to **unplug and replug the dongle**, because existing device nodes sometimes keep their old permissions.



**4. Make sure your panel can show tray icons:**

| Desktop | What to do |
|---|---|
| KDE Plasma, Cinnamon, Cosmic,MATE | Works natively. Check the hidden-icons arrow if you don't see it. |
| XFCE |Modern setup Works natively. If needed add the **Status Notifier Plugin** to the panel (`xfce4-statusnotifier-plugin`): right-click the panel → Panel → Add New Items. |
| GNOME | Install and enable the **AppIndicator** extension (`gnome-shell-extension-appindicator`). Ubuntu ships it enabled. Log out and in if needed. <br> [Click here](https://extensions.gnome.org/extension/615/appindicator-support/) to install using GNOME's official extensions page|

**5. Run the binary:**`mchose-g3-battery-monitor-linux-v*.*.*-amd64-ELF` It asks the startup question once, then the icon appears.

# Using the app

| Menu item | What it does |
|---|---|
| Status | Shows `Connected - Battery: X%`, ` - Battery: X%`, `Mouse: Disconnected / Sleeping`, `Monitor error`, or `Starting...`. |
| Receiver | `Receiver: Connected` or `Receiver: Disconnected` (the USB dongle). |
| Refresh Now | Restarts the query cycle immediately. |
| Launch at Startup | Check / uncheck to add or remove the autostart entry. |
| Low Battery Notification | Check / uncheck to turn the low-battery pop-up on or off. On by default.|
| Quit | Exits the app. |

Command-line flags:

| Flag | Meaning |
|---|---|
| `--debug-log` | Writes a timestamped log to `mchose-g3-debug.log` in the system temp folder (`%TEMP%` on Windows, `/tmp` on Linux). Useful for bug reports. |
| `--minimized` | Accepted and ignored. The autostart entries pass it. The app has no window. |

On Linux, run the binary from a terminal to see the log lines directly. On Windows the release build has no console, so use `--debug-log`.
## Features

- **Battery icon in the tray**: horizontal battery graphic that fills with the charge level.
  - Green at 50% and above, yellow at 20–49%, red below 20%.
  - A warning triangle appears when the mouse is off, asleep, or the receiver is unplugged.
- **Receiver detection**: the dongle is re-scanned on every attempt, so plugging it in after the app has started works.
- **Click menu**: app name, version, status, receiver state, **Refresh Now**, **Launch at Startup**,**Low Battery Notification**, **Quit**.
- **Low-battery notification:** when the battery is 20% or lower, including every time the app starts with a low battery. It fires once and re-arms when the level goes back above 20%. It can be switched off from the tray menu (on by default).
- **Launch at startup**: the app asks once on first run and remembers the answer. The menu item can change it at any time.
- **Hover text** with the percentage (Windows only, see [Known limitations](#known-limitations)).

## Building

Requirements: a recent stable [Rust toolchain](https://rustup.rs).

```bash
cargo build --release
```

The binary is written to `target/release/`.

**Windows** also needs the Visual Studio Build Tools (or Windows SDK) so `rc.exe` is available. It is used to embed the app icon into the `.exe`. With the GNU toolchain, `windres` is used instead.

**Linux** build dependencies:

```bash
install libgtk-3-dev libayatana-appindicator3-dev libudev-dev pkg-config
```

### App icon

Put a square PNG and ico at `assets/` (256×256 or larger looks best).

- **Windows**: `build.rs` takes the `icon.ico` and embeds it in the `.exe`, so Explorer, the taskbar and the startup list show it.
- **Linux**: the PNG is embedded in the binary. On each start the app copies it to `~/.local/share/icons/hicolor/256x256/apps/` and writes a launcher entry to `~/.local/share/applications/mchose-g3-battery-monitor.desktop`, so the app appears in the application menu with its icon. Linux executables cannot carry an icon themselves, so a plain downloaded binary shows a generic icon in the file manager.

The tray icon itself is drawn in code (`src/icon.rs`), not taken from the PNG.

## How it works

The mouse's receiver exposes several HID collections. The app uses the vendor collection with usage page `0xFF02`, usage `0x0002` (VID `0x258A`, PID `0x0138`).

1. **Cached query.** The app sends a feature report starting with `09 0C 09 01 01 00 01 00` and reads the reply. Byte 8 is the last known battery percentage. This only proves the receiver is present, because the receiver answers even when the mouse is off. A value of `0` is treated as unavailable. If the query fails, it retries every 2 seconds.
2. **Live monitoring.** The app then listens for input reports on the same collection:
   - `09 0A 05 <percent>`: live battery level.
   - `09 0A 02 <00 | 01>`: mouse off / sleeping (`00`) or connected (`01`).
3. **Timeout.** If no live report arrives for 55 seconds, the mouse is shown as disconnected / sleeping.
4. **Recovery.** If the HID read fails (for example the dongle is unplugged), the app goes back to step 1 and re-scans the devices.

On Linux the vendor collection may not be reported with the same usage values as on Windows, so the app also tries the other interface-1 nodes and validates the reply prefix before using one.

### Project layout

```
mchose-g3-battery-monitor/
├── Cargo.toml
├── build.rs
├── assets/
│   |── icon.png
|   └── icon.ico
└── src/
    ├── main.rs         # tray, menu, startup prompt, Windows/Linux event loops
    ├── monitor.rs      # HID worker thread (cached query + live reports)
    ├── state.rs        # state shared between worker and UI
    ├── icon.rs         # draws the tray icon
    ├── autostart.rs    # Windows registry / Linux .desktop handling
    └── debuglog.rs     # optional --debug-log file logger
```

## Where settings are stored

| | Windows | Linux |
|---|---|---|
| Autostart entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (value `MCHOSE_G3_Battery_Monitor`) | `~/.config/autostart/mchose-g3-battery-monitor.desktop` |
| Low-battery notification setting | `HKCU\Software\MCHOSE_G3_Battery_Monitor` (`LowBatteryNotifications`) | `~/.config/mchose-g3-battery-monitor/settings.conf` (`low_battery_notifications=true/false`) |
| "Already asked" flag | `HKCU\Software\MCHOSE_G3_Battery_Monitor` (`StartupPromptShown`) | `~/.config/mchose-g3-battery-monitor/settings.conf` |
| Launcher entry | n/a | `~/.local/share/applications/mchose-g3-battery-monitor.desktop` |
| Installed icon | n/a | `~/.local/share/icons/hicolor/256x256/apps/mchose-g3-battery-monitor.png` |

Linux paths follow `XDG_CONFIG_HOME` and `XDG_DATA_HOME` if they are set. If the app is moved, the autostart entry is repointed the next time it starts.

## Uninstalling

- Untick **Launch at Startup** in the tray menu, then quit and delete the executable.
- **Linux:** also remove the launcher entry, the installed icon, the `~/.config/mchose-g3-battery-monitor/` folder, and `/etc/udev/rules.d/70-mchose-g3.rules`.
- **Windows:** optionally delete the `HKCU\Software\MCHOSE_G3_Battery_Monitor` registry key.

## Troubleshooting

| Problem | Cause / fix |
|---|---|
| No tray icon on Linux | Your panel needs AppIndicator support. See [Linux step 4](#linux). |
| Battery can't be read without `sudo` | The udev rule is missing, misnamed, or the dongle wasn't replugged. Use a `70-` file name and replug. |
| `error while loading shared libraries ...` | Install the runtime libraries from [Linux step 1](#linux). |
| Always shows the warning icon | The udev rule is missing, misnamed, or the dongle wasn't replugged. Use a `70-` file name and replug. |
| The startup question never appears again | It is only asked once. Use the **Launch at Startup** menu item, or delete the "already asked" flag from the table above. |
| No low-battery notification | It only fires when a live report shows less than 20%, and only once until the level goes back to 20% or more. Nothing is shown at startup. |
| No low-battery notification | Check that **Low Battery Notification** is ticked in the tray menu. It fires when the battery is 20% or lower: at startup (using the receiver's last known value) and when a live report arrives, then only once until the level goes back above 20%. |
| Icon looks stretched | Check `assets/`|
| Windows still shows the old exe icon | Explorer's icon cache. Rename or copy the exe, or restart Explorer. |

For anything else, run with `--debug-log` and include the log in your report.

## Known limitations

- **No hover % on Linux.** The tray library's tooltip is unsupported with AppIndicator. Status and battery are in the right-click menu instead.
- **No single-instance check.** Starting the app twice gives two tray icons.
- **Designed for the 2.4 GHz receiver.** The HID IDs are fixed to the MCHOSE G3 dongle.
- The app has no window, so it never appears in the dock or taskbar.

## License

Copyright (C) 2026 Binay

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.

The full license text is in the [LICENSE](LICENSE) file.

### Third-party software

This project builds on open-source libraries, mainly under the MIT and Apache-2.0 licenses, which are compatible with the GPL v3. Run `cargo license` to list them.
