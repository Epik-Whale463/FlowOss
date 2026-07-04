//! Local-socket listener — the same protocol the CLI daemon speaks, so
//! `flowoss trigger` keeps working when the desktop app is running. On Linux
//! this is the socket that GNOME's keyboard shortcut talks to; on Windows the
//! desktop app registers global hotkeys directly, but the listener still lets
//! the `flowoss` CLI drive it.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use flowoss_core::ipc;
use interprocess::local_socket::{prelude::*, Listener};

use crate::engine::Command;

pub fn spawn(engine: Sender<Command>) -> Result<()> {
    let listener = bind_taking_over()?;
    std::thread::Builder::new()
        .name("socket-listener".into())
        .spawn(move || listen(listener, engine))
        .context("failed to spawn socket thread")?;
    Ok(())
}

/// Bind the daemon socket. If a CLI daemon already owns it, politely ask it
/// to quit and take over — one dictation service at a time.
fn bind_taking_over() -> Result<Listener> {
    if let Ok(mut existing) = ipc::connect() {
        let _ = writeln!(existing, "quit");
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(200));
            if ipc::connect().is_err() {
                break;
            }
        }
        if ipc::connect().is_ok() {
            bail!(
                "another dictation daemon refuses to release {}",
                ipc::socket_display()
            );
        }
    }
    ipc::bind().with_context(|| format!("failed to bind {}", ipc::socket_display()))
}

fn listen(listener: Listener, engine: Sender<Command>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let mut stream = reader.into_inner();
        let (reply_tx, reply_rx) = channel();
        let command = match line.trim() {
            "toggle" => Command::Toggle(Some(reply_tx)),
            "assist" => Command::AssistToggle(Some(reply_tx)),
            "cancel" => Command::Cancel(Some(reply_tx)),
            "last" => Command::Last(reply_tx),
            "copy-last" | "paste-last" => Command::CopyLast(Some(reply_tx)),
            "quit" => {
                // The desktop app owns the session; hotkey-level quit is not
                // supported (use the tray menu).
                let _ = writeln!(stream, "error: quit the desktop app from its tray menu");
                continue;
            }
            other => {
                let _ = writeln!(stream, "error: unknown command {other:?}");
                continue;
            }
        };
        if engine.send(command).is_err() {
            let _ = writeln!(stream, "error: engine stopped");
            continue;
        }
        let reply = reply_rx
            .recv_timeout(Duration::from_secs(180))
            .unwrap_or_else(|_| "error: engine timeout".into());
        let _ = writeln!(stream, "{reply}");
    }
}
