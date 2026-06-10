//! macOS auto-update + Touch ID sudo support.
//!
//! The actual bundle swap is done by a detached shell script (see
//! `resources/macos_update.sh`) spawned just before the app exits, so the old
//! binary is never locked during the replace. Touch ID for sudo is enabled by a
//! second privileged script run once via the native admin dialog.
//!
//! Everything the scripts rely on ships with base macOS — no Xcode CLT needed.

/// `(os, arch)` for the frontend to pick the right release asset.
/// os ∈ {"macos","windows","linux"}, arch ∈ {"aarch64","x86_64",...}.
#[tauri::command]
pub fn get_platform_info() -> (String, String) {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    };
    (os.to_string(), std::env::consts::ARCH.to_string())
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::PathBuf;
    use std::process::Command;

    const UPDATE_SCRIPT: &str = include_str!("../resources/macos_update.sh");
    const ENABLE_TOUCHID_SCRIPT: &str =
        include_str!("../resources/macos_enable_touchid_sudo.sh");

    /// Resolve the running `.app` bundle from the executable path
    /// (`…/Magnolia.app/Contents/MacOS/bin` → `…/Magnolia.app`).
    pub fn current_app_bundle() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        exe.ancestors()
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                "not running from a .app bundle (update only works on an installed app)"
                    .to_string()
            })
    }

    /// Write an embedded script to a temp file and mark it executable.
    fn write_temp_script(name: &str, contents: &str) -> Result<PathBuf, String> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).map_err(|e| format!("write {name}: {e}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod {name}: {e}"))?;
        Ok(path)
    }

    /// True if pam_tid is enabled (uncommented) in either sudo PAM config.
    pub fn is_touch_id_sudo_enabled() -> bool {
        for f in ["/etc/pam.d/sudo_local", "/etc/pam.d/sudo"] {
            if let Ok(contents) = std::fs::read_to_string(f) {
                for line in contents.lines() {
                    let t = line.trim_start();
                    if t.starts_with('#') {
                        continue;
                    }
                    if t.contains("pam_tid.so") && t.contains("auth") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Enable Touch ID for sudo via a one-time privileged script (native admin
    /// dialog, which itself supports Touch ID on capable Macs). Idempotent.
    pub fn enable_touch_id_sudo() -> Result<(), String> {
        if is_touch_id_sudo_enabled() {
            return Ok(());
        }
        let script = write_temp_script(
            "magnolia_enable_touchid_sudo.sh",
            ENABLE_TOUCHID_SCRIPT,
        )?;
        // AppleScript string literal: escape backslashes then double-quotes.
        let inner = format!("/bin/bash '{}'", script.display());
        let escaped = inner.replace('\\', "\\\\").replace('"', "\\\"");
        let applescript =
            format!("do shell script \"{escaped}\" with administrator privileges");

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .output()
            .map_err(|e| format!("failed to launch osascript: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&output.stderr);
            if err.contains("-128") {
                Err("authorization cancelled".to_string())
            } else {
                Err(format!("failed to enable Touch ID for sudo: {}", err.trim()))
            }
        }
    }

    /// Spawn the detached update helper and exit so the swap can proceed.
    pub fn install(zip_path: String) -> Result<(), String> {
        use std::process::Stdio;

        let app = current_app_bundle()?;
        if !std::path::Path::new(&zip_path).exists() {
            return Err(format!("downloaded file missing: {zip_path}"));
        }
        let script = write_temp_script("magnolia_update.sh", UPDATE_SCRIPT)?;
        let pid = std::process::id();

        Command::new("/bin/bash")
            .arg(&script)
            .arg(&zip_path)
            .arg(&app)
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn update helper: {e}"))?;

        // Let the helper detach, then quit hard so the binary unlocks and the
        // helper's PID-wait completes.
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn is_touch_id_sudo_enabled() -> bool {
    imp::is_touch_id_sudo_enabled()
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn enable_touch_id_sudo() -> Result<(), String> {
    tokio::task::spawn_blocking(imp::enable_touch_id_sudo)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
pub fn install_macos(zip_path: String) -> Result<(), String> {
    imp::install(zip_path)
}

// ── Non-macOS stubs so the command handlers compile everywhere ───────────────

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn is_touch_id_sudo_enabled() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn enable_touch_id_sudo() -> Result<(), String> {
    Err("Touch ID sudo is only available on macOS".to_string())
}
