//! FlowOSS desktop shell (milestone M3): tray icon, status overlay, and
//! settings window around the dictation engine.

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod engine;
mod hotkey;
mod settings;
mod socket;

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::Duration;

use engine::Command;
use serde::Serialize;
use settings::Settings;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

struct EngineHandle(Mutex<Sender<Command>>);

impl EngineHandle {
    fn send(&self, command: Command) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .send(command)
            .map_err(|_| "dictation engine stopped".into())
    }

    fn ask(&self, make: impl FnOnce(Sender<String>) -> Command) -> Result<String, String> {
        let (tx, rx) = channel();
        self.send(make(tx))?;
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| "engine timeout".into())
    }
}

#[tauri::command]
fn ui_log(message: String) {
    eprintln!("[ui] {message}");
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

#[tauri::command]
fn set_settings(engine: tauri::State<EngineHandle>, new: Settings) -> Result<(), String> {
    new.save().map_err(|e| e.to_string())?;
    engine.send(Command::UpdateSettings(new))
}

#[tauri::command]
fn list_microphones() -> Vec<String> {
    flowoss_audio::list_input_devices().unwrap_or_default()
}

#[tauri::command]
fn toggle_dictation(engine: tauri::State<EngineHandle>) -> Result<String, String> {
    engine.ask(|tx| Command::Toggle(Some(tx)))
}

#[tauri::command]
fn last_transcript(engine: tauri::State<EngineHandle>) -> Result<String, String> {
    engine.ask(Command::Last)
}

#[tauri::command]
fn copy_last(engine: tauri::State<EngineHandle>) -> Result<String, String> {
    engine.ask(|tx| Command::CopyLast(Some(tx)))
}

#[derive(Serialize)]
struct ModelStatus {
    path: String,
    ready: bool,
    size_mb: u64,
}

#[tauri::command]
fn model_status() -> ModelStatus {
    let settings = Settings::load();
    let ready = settings.model_dir.join("tokens.txt").exists();
    let size_mb = std::fs::read_dir(&settings.model_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum::<u64>()
                / (1024 * 1024)
        })
        .unwrap_or(0);
    ModelStatus {
        path: settings.model_dir.display().to_string(),
        ready,
        size_mb,
    }
}

#[tauri::command]
fn hotkey_binding() -> Option<String> {
    hotkey::current_binding()
}

#[tauri::command]
fn set_hotkey_binding(binding: String) -> Result<(), String> {
    let trigger = format!("{} trigger", cli_binary_path());
    hotkey::set_binding(&binding, &trigger)
}

/// Path of the `flowoss` CLI used by the desktop hotkey. Prefer the
/// installed copy; fall back to one next to this executable.
fn cli_binary_path() -> String {
    let installed = dirs_home().join(".local/bin/flowoss");
    if installed.exists() {
        return installed.display().to_string();
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("flowoss")))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "flowoss".into())
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(Into::into)
        .unwrap_or_else(|| "/".into())
}

fn build_windows(app: &AppHandle) -> tauri::Result<()> {
    // Status overlay: a small pill that must never steal focus (PRD 11.7).
    // Click-through (ignore cursor events) is applied by the engine after
    // the first show — calling it on an unrealized GTK window panics in tao.
    WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
        .title("FlowOSS")
        .inner_size(320.0, 84.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;

    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("FlowOSS Settings")
        .inner_size(760.0, 640.0)
        .min_inner_size(560.0, 480.0)
        .visible(false)
        .build()?;
    Ok(())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Start/stop dictation", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit FlowOSS", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &settings, &quit])?;

    TrayIconBuilder::with_id("flowoss-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("FlowOSS — local dictation")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                let engine = app.state::<EngineHandle>();
                let _ = engine.send(Command::Toggle(None));
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    // GNOME Wayland doesn't let apps position their own windows, which
    // breaks the bottom-anchored overlay. XWayland honors positioning,
    // always-on-top, and click-through, so prefer the x11 backend unless
    // the user overrides it.
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            build_windows(&handle)?;
            build_tray(&handle)?;

            let engine_tx = engine::spawn(handle, Settings::load());
            socket::spawn(engine_tx.clone())?;
            app.manage(EngineHandle(Mutex::new(engine_tx)));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window hides it; the app lives in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ui_log,
            get_settings,
            set_settings,
            list_microphones,
            toggle_dictation,
            last_transcript,
            copy_last,
            model_status,
            hotkey_binding,
            set_hotkey_binding,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run FlowOSS");
}
