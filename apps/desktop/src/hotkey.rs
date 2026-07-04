//! Global hotkey binding, platform-dispatched.
//!
//! Linux/Wayland can't grab global shortcuts from an app, so bindings live in
//! GNOME settings and run `flowoss trigger` (see `hotkey_linux`). Windows lets
//! the app register global hotkeys directly via the Tauri global-shortcut
//! plugin (see `hotkey_windows`).

#[cfg_attr(target_os = "linux", path = "hotkey_linux.rs")]
#[cfg_attr(windows, path = "hotkey_windows.rs")]
#[cfg_attr(not(any(target_os = "linux", windows)), path = "hotkey_stub.rs")]
mod imp;

pub use imp::*;
