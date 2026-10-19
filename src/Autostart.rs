//! Start-at-login handling, plus remembering that the user was already asked.
//!
//! Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run  (autostart)
//!          HKCU\Software\MCHOSE_G3_Battery_Monitor             (prompt memory)
//! Linux:   ~/.config/autostart/mchose-g3-battery-monitor.desktop (autostart)
//!          ~/.config/mchose-g3-battery-monitor/settings.conf     (prompt memory)
//!          ~/.local/share/applications/mchose-g3-battery-monitor.desktop (launcher entry)

pub use imp::{
    install_launcher, is_enabled, mark_prompt_shown, prompt_shown, set_enabled, sync_path,
};

// ======================================================================
// Windows
// ======================================================================
#[cfg(target_os = "windows")]
mod imp {
    use std::env;
    use std::io::ErrorKind;
    use winreg::{enums::*, RegKey};

    const APP_ID: &str = "MCHOSE_G3_Battery_Monitor";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const SETTINGS_KEY: &str = r"Software\MCHOSE_G3_Battery_Monitor";
    const PROMPT_VALUE: &str = "StartupPromptShown";

    fn command() -> Result<String, String> {
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        Ok(format!("\"{}\" --minimized", exe.display()))
    }

    fn current_value() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_READ)
            .ok()?
            .get_value::<String, _>(APP_ID)
            .ok()
    }

    pub fn is_enabled() -> bool {
        current_value().is_some()
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        if enabled {
            let cmd = command()?;
            let (run, _) = hkcu.create_subkey(RUN_KEY).map_err(|e| e.to_string())?;
            run.set_value(APP_ID, &cmd).map_err(|e| e.to_string())
        } else {
            let run = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
                Ok(run) => run,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.to_string()),
            };
            match run.delete_value(APP_ID) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    /// If autostart is on but points at an old exe location, repoint it here.
    pub fn sync_path() {
        if let (Some(current), Ok(wanted)) = (current_value(), command()) {
            if current != wanted {
                let _ = set_enabled(true);
            }
        }
    }

    /// Linux-only feature (application launcher entry); nothing to do on Windows.
    pub fn install_launcher() {}

    pub fn prompt_shown() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(SETTINGS_KEY, KEY_READ)
            .ok()
            .and_then(|k| k.get_value::<u32, _>(PROMPT_VALUE).ok())
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    pub fn mark_prompt_shown() {
        if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(SETTINGS_KEY) {
            let _ = key.set_value(PROMPT_VALUE, &1u32);
        }
    }
}

// ======================================================================
// Linux
// ======================================================================
#[cfg(target_os = "linux")]
mod imp {
    use std::env;
    use std::fs;
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

    const DESKTOP_FILE_NAME: &str = "mchose-g3-battery-monitor.desktop";
    const SETTINGS_DIR: &str = "mchose-g3-battery-monitor";
    const SETTINGS_FILE: &str = "settings.conf";
    const PROMPT_LINE: &str = "startup_prompt_shown=true";

    // App icon, embedded in the binary and copied to the user's icon folder.
    const ICON_FILE_NAME: &str = "mchose-g3-battery-monitor.png";
    const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

    /// $XDG_CONFIG_HOME, or ~/.config
    fn config_home() -> Option<PathBuf> {
        match env::var_os("XDG_CONFIG_HOME") {
            Some(p) if !p.is_empty() && PathBuf::from(&p).is_absolute() => Some(p.into()),
            _ => env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".config")),
        }
    }

    /// $XDG_DATA_HOME, or ~/.local/share
    fn data_home() -> Option<PathBuf> {
        match env::var_os("XDG_DATA_HOME") {
            Some(p) if !p.is_empty() && PathBuf::from(&p).is_absolute() => Some(p.into()),
            _ => env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".local").join("share")),
        }
    }

    fn icon_path() -> Option<PathBuf> {
        data_home().map(|d| {
            d.join("icons")
                .join("hicolor")
                .join("256x256")
                .join("apps")
                .join(ICON_FILE_NAME)
        })
    }

    /// Copies the embedded icon to the user's icon folder (if needed).
    fn install_icon() -> Option<PathBuf> {
        let path = icon_path()?;
        let up_to_date = fs::read(&path).map(|b| b == ICON_PNG).unwrap_or(false);
        if !up_to_date {
            fs::create_dir_all(path.parent()?).ok()?;
            fs::write(&path, ICON_PNG).ok()?;
        }
        Some(path)
    }

    fn desktop_path() -> Option<PathBuf> {
        config_home().map(|c| c.join("autostart").join(DESKTOP_FILE_NAME))
    }

    fn settings_path() -> Option<PathBuf> {
        config_home().map(|c| c.join(SETTINGS_DIR).join(SETTINGS_FILE))
    }

    /// Quote the executable path for the Exec= key of a .desktop file.
    fn exec_quote(path: &str) -> String {
        let mut out = String::from("\"");
        for c in path.chars() {
            match c {
                // Exec-level escape (\), then doubled again at string level.
                '\\' => out.push_str("\\\\\\\\"),
                '"' | '`' | '$' => {
                    out.push_str("\\\\");
                    out.push(c);
                }
                '%' => out.push_str("%%"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn exec_line(minimized: bool) -> Result<String, String> {
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        let suffix = if minimized { " --minimized" } else { "" };
        Ok(format!("Exec={}{}", exec_quote(&exe.to_string_lossy()), suffix))
    }

    /// Text of a .desktop file. `autostart` = true for ~/.config/autostart,
    /// false for the application-launcher entry.
    fn desktop_entry(icon: Option<&Path>, autostart: bool) -> Result<String, String> {
        let mut text = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=MCHOSE G3 Battery Monitor\n\
             Comment=Battery monitor for the MCHOSE G3 mouse\n\
             {}\n",
            exec_line(autostart)?
        );
        if let Some(icon) = icon {
            text.push_str(&format!("Icon={}\n", icon.display()));
        }
        text.push_str("Terminal=false\n");
        if autostart {
            text.push_str("X-GNOME-Autostart-enabled=true\n");
        } else {
            text.push_str("Categories=Utility;\nStartupNotify=false\n");
        }
        Ok(text)
    }

    fn launcher_path() -> Option<PathBuf> {
        data_home().map(|d| d.join("applications").join(DESKTOP_FILE_NAME))
    }

    /// Keeps ~/.local/share/applications/<app>.desktop (with the icon) up to date,
    /// so the app shows up in the launcher. Safe to call on every start.
    pub fn install_launcher() {
        let (Some(path), Some(icon)) = (launcher_path(), install_icon()) else {
            return;
        };
        let Ok(wanted) = desktop_entry(Some(&icon), false) else {
            return;
        };
        if fs::read_to_string(&path).map(|t| t == wanted).unwrap_or(false) {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&path, wanted);
    }

    pub fn is_enabled() -> bool {
        let Some(path) = desktop_path() else {
            return false;
        };
        match fs::read_to_string(path) {
            // Desktop environments disable an entry by adding one of these keys.
            Ok(text) => !text.lines().any(|l| {
                matches!(l.trim(), "Hidden=true" | "X-GNOME-Autostart-enabled=false")
            }),
            Err(_) => false,
        }
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let path = desktop_path().ok_or("cannot locate the config directory")?;

        if enabled {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let icon = install_icon();
            fs::write(&path, desktop_entry(icon.as_deref(), true)?).map_err(|e| e.to_string())
        } else {
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    /// If autostart is on but Exec= points at an old location, rewrite it.
    pub fn sync_path() {
        if !is_enabled() {
            return;
        }
        let (Some(path), Ok(wanted)) = (desktop_path(), exec_line(true)) else {
            return;
        };
        let up_to_date = fs::read_to_string(&path)
            .map(|text| {
                text.lines().any(|l| l == wanted) && text.lines().any(|l| l.starts_with("Icon="))
            })
            .unwrap_or(false);
        if !up_to_date {
            let _ = set_enabled(true);
        }
    }

    pub fn prompt_shown() -> bool {
        settings_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|text| text.lines().any(|l| l.trim() == PROMPT_LINE))
            .unwrap_or(false)
    }

    pub fn mark_prompt_shown() {
        if let Some(path) = settings_path() {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(path, format!("{PROMPT_LINE}\n"));
        }
    }
}