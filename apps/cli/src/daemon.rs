//! Dictation daemon (milestone M2).
//!
//! Keeps the STT model warm and listens on a unix socket for commands sent
//! by `flowoss trigger` (bound to a desktop keyboard shortcut). First
//! trigger starts recording; the second stops, transcribes, and inserts the
//! text via clipboard (+ optional simulated paste).
//!
//! The cpal stream is not `Send`, so all state lives on this thread and
//! commands are serialized through the socket accept loop.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use flowoss_insertion::{notify, InsertOutcome, Inserter, PasteMode};
use flowoss_text_cleanup::CleanupMode;

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    runtime.join("flowoss.sock")
}

fn last_transcript_path() -> PathBuf {
    flowoss_core::data_dir().join("last_transcript.txt")
}

pub struct DaemonOptions {
    pub device: Option<String>,
    pub paste_mode: PasteMode,
    pub cleanup: CleanupMode,
}

struct Daemon {
    stt: flowoss_stt::Transcriber,
    vad: flowoss_vad::SpeechDetector,
    inserter: Inserter,
    options: DaemonOptions,
    recording: Option<flowoss_audio::Recording>,
    last_transcript: String,
}

pub fn run(
    stt: flowoss_stt::Transcriber,
    vad: flowoss_vad::SpeechDetector,
    options: DaemonOptions,
) -> Result<()> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        bail!("another flowoss daemon is already running on {}", path.display());
    }
    let _ = std::fs::remove_file(&path); // stale socket from a crashed run
    let listener =
        UnixListener::bind(&path).with_context(|| format!("failed to bind {}", path.display()))?;

    let mut daemon = Daemon {
        stt,
        vad,
        inserter: Inserter::new()?,
        options,
        recording: None,
        last_transcript: std::fs::read_to_string(last_transcript_path()).unwrap_or_default(),
    };

    eprintln!("flowoss daemon ready on {}", path.display());
    eprintln!("Bind a keyboard shortcut to: flowoss trigger");

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("socket accept error: {e}");
                continue;
            }
        };
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let command = line.trim();
        let reply = match command {
            "toggle" => daemon.toggle(),
            "cancel" => daemon.cancel(),
            "last" => Ok(daemon.last_transcript.clone()),
            // The daemon must own the clipboard: on Wayland the contents die
            // with the process that set them, so a short-lived CLI can't copy.
            "copy-last" => daemon.copy_last(),
            "paste-last" => daemon.paste_last(),
            "quit" => {
                let _ = writeln!(stream, "ok bye");
                break;
            }
            other => Ok(format!("error: unknown command {other:?}")),
        };
        let reply = reply.unwrap_or_else(|e| {
            notify("FlowOSS error", &e.to_string());
            format!("error: {e}")
        });
        let _ = writeln!(stream, "{reply}");
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

impl Daemon {
    fn toggle(&mut self) -> Result<String> {
        match self.recording.take() {
            None => {
                let recording = flowoss_audio::Recording::start(self.options.device.as_deref())?;
                self.recording = Some(recording);
                notify("FlowOSS", "Recording... trigger again to stop");
                Ok("recording".into())
            }
            Some(recording) => {
                let samples = recording.stop();
                notify("FlowOSS", "Transcribing...");
                let start = Instant::now();
                let Some(speech) = self.vad.extract_speech(&samples) else {
                    notify("FlowOSS", "No speech detected");
                    return Ok("no speech".into());
                };
                let text = self.stt.transcribe(&speech);
                if text.is_empty() {
                    notify("FlowOSS", "No speech detected");
                    return Ok("no speech".into());
                }
                let text = flowoss_text_cleanup::clean(&text, self.options.cleanup);
                eprintln!(
                    "[{:.2}s audio -> {} chars in {:.2}s]",
                    samples.len() as f32 / flowoss_core::SAMPLE_RATE as f32,
                    text.len(),
                    start.elapsed().as_secs_f32()
                );

                self.last_transcript = text.clone();
                let _ = std::fs::create_dir_all(flowoss_core::data_dir());
                let _ = std::fs::write(last_transcript_path(), &text);

                match self.inserter.insert(&text, self.options.paste_mode) {
                    Ok(InsertOutcome::Pasted) => notify("FlowOSS ✓ pasted", &text),
                    Ok(InsertOutcome::Copied) => {
                        notify("FlowOSS ✓ copied — press Ctrl+V", &text)
                    }
                    Err(e) => notify("FlowOSS insertion failed", &e.to_string()),
                }
                Ok(text)
            }
        }
    }

    fn copy_last(&mut self) -> Result<String> {
        if self.last_transcript.is_empty() {
            return Ok("no transcript yet".into());
        }
        let text = self.last_transcript.clone();
        self.inserter.insert(&text, PasteMode::Copy)?;
        notify("FlowOSS ✓ copied — press Ctrl+V", &text);
        Ok(text)
    }

    fn paste_last(&mut self) -> Result<String> {
        if self.last_transcript.is_empty() {
            return Ok("no transcript yet".into());
        }
        let text = self.last_transcript.clone();
        match self.inserter.insert(&text, PasteMode::Auto)? {
            InsertOutcome::Pasted => notify("FlowOSS ✓ pasted", &text),
            InsertOutcome::Copied => notify("FlowOSS ✓ copied — press Ctrl+V", &text),
        }
        Ok(text)
    }

    fn cancel(&mut self) -> Result<String> {
        if self.recording.take().is_some() {
            notify("FlowOSS", "Recording cancelled");
            Ok("cancelled".into())
        } else {
            Ok("idle".into())
        }
    }
}

/// Send a command to the running daemon and return its reply.
pub fn send_command(command: &str) -> Result<String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).with_context(|| {
        format!(
            "cannot reach flowoss daemon at {} — start it with `flowoss daemon`",
            path.display()
        )
    })?;
    writeln!(stream, "{command}")?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    Ok(reply.trim_end().to_string())
}
