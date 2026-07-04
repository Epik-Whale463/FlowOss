//! Windows global hotkeys via `tauri-plugin-global-shortcut`.
//!
//! The app registers dictation and assist accelerators itself and drives the
//! dictation engine directly when they fire — no desktop-environment keybinding
//! and no `flowoss trigger` round-trip needed. Bindings persist to
//! `hotkeys.toml` next to the main config so they survive restarts.

use std::str::FromStr;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::engine::Command;
use crate::EngineHandle;

const DEFAULT_DICTATION: &str = "Ctrl+Shift+Space";
const DEFAULT_ASSIST: &str = "Ctrl+Shift+A";

#[derive(Copy, Clone, PartialEq, Eq)]
enum Kind {
    Dictation,
    Assist,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct HotkeyConfig {
    dictation: String,
    assist: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            dictation: DEFAULT_DICTATION.into(),
            assist: DEFAULT_ASSIST.into(),
        }
    }
}

fn config_file() -> std::path::PathBuf {
    flowoss_core::config_path().with_file_name("hotkeys.toml")
}

impl HotkeyConfig {
    fn load() -> Self {
        std::fs::read_to_string(config_file())
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let path = config_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }
}

/// Currently registered accelerators and what they trigger.
fn registered() -> &'static Mutex<Vec<(Shortcut, Kind)>> {
    static R: OnceLock<Mutex<Vec<(Shortcut, Kind)>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

/// The global-shortcut plugin, wired to drive the engine on key press.
pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let kind = registered()
                .lock()
                .unwrap()
                .iter()
                .find(|(s, _)| s == shortcut)
                .map(|(_, k)| *k);
            match kind {
                Some(Kind::Dictation) => {
                    let _ = app.state::<EngineHandle>().send(Command::Toggle(None));
                }
                Some(Kind::Assist) => {
                    let _ = app.state::<EngineHandle>().send(Command::AssistToggle(None));
                }
                None => {}
            }
        })
        .build()
}

/// (Re)register the dictation and assist accelerators from the saved config.
///
/// Best-effort per accelerator: if one is already claimed by another app, we
/// still register the other and report the conflicts, rather than leaving the
/// user with no working hotkeys. The successfully bound ones are what fire.
pub fn register_all(app: &AppHandle) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let cfg = HotkeyConfig::load();
    let mut map = Vec::new();
    let mut errors = Vec::new();
    for (accel, kind) in [
        (cfg.dictation.trim(), Kind::Dictation),
        (cfg.assist.trim(), Kind::Assist),
    ] {
        if accel.is_empty() {
            continue;
        }
        let shortcut = match Shortcut::from_str(accel) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("invalid shortcut {accel:?}: {e}"));
                continue;
            }
        };
        match gs.register(shortcut) {
            Ok(()) => map.push((shortcut, kind)),
            Err(e) => errors.push(format!("{accel:?} unavailable ({e})")),
        }
    }
    *registered().lock().unwrap() = map;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn current_binding() -> Option<String> {
    let b = HotkeyConfig::load().dictation;
    (!b.trim().is_empty()).then_some(b)
}

pub fn current_assist_binding() -> Option<String> {
    let b = HotkeyConfig::load().assist;
    (!b.trim().is_empty()).then_some(b)
}

/// `trigger_command` is unused on Windows (the app fires the engine itself);
/// it's kept for signature parity with the Linux backend.
pub fn set_binding(binding: &str, _trigger_command: &str) -> Result<(), String> {
    let mut cfg = HotkeyConfig::load();
    cfg.dictation = binding.to_string();
    cfg.save()
}

pub fn set_assist_binding(binding: &str, _trigger_command: &str) -> Result<(), String> {
    let mut cfg = HotkeyConfig::load();
    cfg.assist = binding.to_string();
    cfg.save()
}
