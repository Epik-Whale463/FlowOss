//! GNOME hotkey binding via gsettings.
//!
//! Wayland compositors don't allow apps to grab global shortcuts directly,
//! so the binding lives in GNOME's own keyboard settings and runs
//! `flowoss trigger`, which reaches us over the unix socket.

use std::process::Command;

const KB_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/flowoss/";
const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";

fn gsettings(args: &[&str]) -> Option<String> {
    let out = Command::new("gsettings").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Current binding (e.g. `<Super>z`), if the FlowOSS shortcut exists.
pub fn current_binding() -> Option<String> {
    let value = gsettings(&[
        "get",
        &format!("{SCHEMA}.custom-keybinding:{KB_PATH}"),
        "binding",
    ])?;
    let binding = value.trim_matches('\'').to_string();
    (!binding.is_empty()).then_some(binding)
}

/// Create or update the FlowOSS shortcut to run `trigger_command`.
pub fn set_binding(binding: &str, trigger_command: &str) -> Result<(), String> {
    // Preserve any other custom keybindings in the list.
    let list = gsettings(&["get", SCHEMA, "custom-keybindings"]).unwrap_or_else(|| "[]".into());
    if !list.contains(KB_PATH) {
        let entries = list.trim_start_matches("@as").trim();
        let inner = entries.trim_start_matches('[').trim_end_matches(']').trim();
        let new_list = if inner.is_empty() {
            format!("['{KB_PATH}']")
        } else {
            format!("[{inner}, '{KB_PATH}']")
        };
        gsettings(&["set", SCHEMA, "custom-keybindings", &new_list])
            .ok_or("failed to update keybinding list")?;
    }
    let schema_path = format!("{SCHEMA}.custom-keybinding:{KB_PATH}");
    gsettings(&["set", &schema_path, "name", "FlowOSS dictation toggle"])
        .ok_or("failed to set name")?;
    gsettings(&["set", &schema_path, "command", trigger_command])
        .ok_or("failed to set command")?;
    gsettings(&["set", &schema_path, "binding", binding]).ok_or("failed to set binding")?;
    Ok(())
}
