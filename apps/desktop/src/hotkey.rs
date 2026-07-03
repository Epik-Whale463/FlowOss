//! GNOME hotkey binding via gsettings.
//!
//! Wayland compositors don't allow apps to grab global shortcuts directly,
//! so the binding lives in GNOME's own keyboard settings and runs
//! `flowoss trigger`, which reaches us over the unix socket.

use std::process::Command;

const DICTATION_KB_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/flowoss/";
const ASSIST_KB_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/flowoss-assist/";
const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";

fn gsettings(args: &[&str]) -> Option<String> {
    let out = Command::new("gsettings").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Current binding (e.g. `<Super>z`), if the FlowOSS shortcut exists.
pub fn current_binding() -> Option<String> {
    current_binding_for(DICTATION_KB_PATH)
}

/// Current assist binding (e.g. `<Super>x`), if it exists.
pub fn current_assist_binding() -> Option<String> {
    current_binding_for(ASSIST_KB_PATH)
}

fn current_binding_for(path: &str) -> Option<String> {
    let value = gsettings(&[
        "get",
        &format!("{SCHEMA}.custom-keybinding:{path}"),
        "binding",
    ])?;
    let binding = value.trim_matches('\'').to_string();
    (!binding.is_empty()).then_some(binding)
}

/// Create or update the FlowOSS shortcut to run `trigger_command`.
pub fn set_binding(binding: &str, trigger_command: &str) -> Result<(), String> {
    set_binding_for(DICTATION_KB_PATH, "FlowOSS dictation toggle", binding, trigger_command)
}

/// Create or update the FlowOSS Assist shortcut to run `trigger_command`.
pub fn set_assist_binding(binding: &str, trigger_command: &str) -> Result<(), String> {
    set_binding_for(ASSIST_KB_PATH, "FlowOSS assist", binding, trigger_command)
}

fn set_binding_for(
    path: &str,
    name: &str,
    binding: &str,
    trigger_command: &str,
) -> Result<(), String> {
    // Preserve any other custom keybindings in the list.
    let list = gsettings(&["get", SCHEMA, "custom-keybindings"]).unwrap_or_else(|| "[]".into());
    if !list.contains(path) {
        let entries = list.trim_start_matches("@as").trim();
        let inner = entries.trim_start_matches('[').trim_end_matches(']').trim();
        let new_list = if inner.is_empty() {
            format!("['{path}']")
        } else {
            format!("[{inner}, '{path}']")
        };
        gsettings(&["set", SCHEMA, "custom-keybindings", &new_list])
            .ok_or("failed to update keybinding list")?;
    }
    let schema_path = format!("{SCHEMA}.custom-keybinding:{path}");
    gsettings(&["set", &schema_path, "name", name]).ok_or("failed to set name")?;
    gsettings(&["set", &schema_path, "command", trigger_command])
        .ok_or("failed to set command")?;
    gsettings(&["set", &schema_path, "binding", binding]).ok_or("failed to set binding")?;
    Ok(())
}
