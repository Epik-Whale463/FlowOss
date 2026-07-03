//! Dictation engine: a dedicated thread that owns the microphone stream,
//! the warm STT/VAD models, and the clipboard.
//!
//! The cpal recording handle is not `Send`, so everything audio-related
//! lives here; the rest of the app talks to it through a channel. While
//! recording, the loop ticks every 80 ms to stream live mic levels to the
//! overlay.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use flowoss_insertion::{notify, InsertOutcome, Inserter};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::Settings;

pub enum Command {
    Toggle(Option<Sender<String>>),
    Cancel(Option<Sender<String>>),
    Last(Sender<String>),
    CopyLast(Option<Sender<String>>),
    UpdateSettings(Settings),
}

/// Event payload for the overlay and settings UIs.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum StateEvent {
    Loading,
    Idle,
    Recording { level: f32, secs: f32 },
    Processing,
    Success { text: String, pasted: bool },
    NoSpeech,
    Error { message: String },
}

const STATE_EVENT: &str = "flowoss://state";
/// How long success/error stays on screen before the overlay hides.
const LINGER: Duration = Duration::from_millis(1600);

pub fn spawn(app: AppHandle, settings: Settings) -> Sender<Command> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("dictation-engine".into())
        .spawn(move || run(app, settings, rx))
        .expect("failed to spawn engine thread");
    tx
}

struct Engine {
    app: AppHandle,
    settings: Settings,
    stt: Option<flowoss_stt::Transcriber>,
    vad: Option<flowoss_vad::SpeechDetector>,
    inserter: Option<Inserter>,
    recording: Option<flowoss_audio::Recording>,
    last_transcript: String,
    hide_at: Option<Instant>,
}

fn run(app: AppHandle, settings: Settings, rx: Receiver<Command>) {
    let mut engine = Engine {
        app,
        settings,
        stt: None,
        vad: None,
        inserter: None,
        recording: None,
        last_transcript: std::fs::read_to_string(flowoss_core::last_transcript_path())
            .unwrap_or_default(),
        hide_at: None,
    };
    engine.emit(StateEvent::Loading);
    engine.load_models();
    engine.emit(StateEvent::Idle);

    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(command) => engine.handle(command),
            Err(RecvTimeoutError::Timeout) => engine.tick(),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

impl Engine {
    fn emit(&self, event: StateEvent) {
        let _ = self.app.emit(STATE_EVENT, &event);
    }

    fn overlay_visible(&self, visible: bool) {
        match self.app.get_webview_window("overlay") {
            Some(window) => {
                let result = if visible { window.show() } else { window.hide() };
                if let Err(e) = result {
                    eprintln!("overlay {}: {e}", if visible { "show" } else { "hide" });
                }
            }
            None => eprintln!("overlay window missing"),
        }
    }

    fn load_models(&mut self) {
        match flowoss_stt::Transcriber::from_model_dir(&self.settings.model_dir, self.settings.threads)
        {
            Ok(stt) => self.stt = Some(stt),
            Err(e) => {
                self.stt = None;
                eprintln!("STT load failed: {e}");
            }
        }
        match flowoss_vad::SpeechDetector::new(&self.settings.vad_model) {
            Ok(vad) => self.vad = Some(vad),
            Err(e) => {
                self.vad = None;
                eprintln!("VAD load failed: {e}");
            }
        }
        if self.inserter.is_none() {
            self.inserter = Inserter::new().ok();
        }
    }

    fn tick(&mut self) {
        if let Some(recording) = &self.recording {
            self.emit(StateEvent::Recording {
                level: recording.level(),
                secs: recording.duration_secs(),
            });
        } else if let Some(hide_at) = self.hide_at {
            if Instant::now() >= hide_at {
                self.hide_at = None;
                self.overlay_visible(false);
                self.emit(StateEvent::Idle);
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Toggle(reply) => {
                let result = self.toggle();
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            Command::Cancel(reply) => {
                let result = self.cancel();
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            Command::Last(reply) => {
                let _ = reply.send(self.last_transcript.clone());
            }
            Command::CopyLast(reply) => {
                let result = self.copy_last();
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            Command::UpdateSettings(new) => {
                let reload = new.model_dir != self.settings.model_dir
                    || new.vad_model != self.settings.vad_model
                    || new.threads != self.settings.threads;
                self.settings = new;
                if reload {
                    self.emit(StateEvent::Loading);
                    self.load_models();
                    self.emit(StateEvent::Idle);
                }
            }
        }
    }

    fn toggle(&mut self) -> String {
        match self.recording.take() {
            None => self.start_recording(),
            Some(recording) => self.finish_recording(recording),
        }
    }

    fn start_recording(&mut self) -> String {
        if self.stt.is_none() {
            let message = format!(
                "Model not loaded — check model directory {}",
                self.settings.model_dir.display()
            );
            self.show_error(&message);
            return format!("error: {message}");
        }
        match flowoss_audio::Recording::start(self.settings.device.as_deref()) {
            Ok(recording) => {
                self.recording = Some(recording);
                self.hide_at = None;
                self.overlay_visible(true);
                self.emit(StateEvent::Recording { level: 0.0, secs: 0.0 });
                "recording".into()
            }
            Err(e) => {
                let message = format!("Microphone error: {e}");
                self.show_error(&message);
                format!("error: {message}")
            }
        }
    }

    fn finish_recording(&mut self, recording: flowoss_audio::Recording) -> String {
        let samples = recording.stop();
        self.emit(StateEvent::Processing);
        self.overlay_visible(true);

        let speech = match self.vad.as_mut() {
            Some(vad) => vad.extract_speech(&samples),
            None => Some(samples),
        };
        let Some(speech) = speech else {
            self.emit(StateEvent::NoSpeech);
            self.hide_at = Some(Instant::now() + LINGER);
            return "no speech".into();
        };

        let Some(stt) = self.stt.as_mut() else {
            self.show_error("Model not loaded");
            return "error: model not loaded".into();
        };
        let text = stt.transcribe(&speech);
        if text.is_empty() {
            self.emit(StateEvent::NoSpeech);
            self.hide_at = Some(Instant::now() + LINGER);
            return "no speech".into();
        }
        let text = flowoss_text_cleanup::clean(&text, self.settings.cleanup_mode());

        self.last_transcript = text.clone();
        let _ = std::fs::create_dir_all(flowoss_core::data_dir());
        let _ = std::fs::write(flowoss_core::last_transcript_path(), &text);

        let pasted = match self.insert(&text) {
            Ok(InsertOutcome::Pasted) => true,
            Ok(InsertOutcome::Copied) => false,
            Err(e) => {
                self.show_error(&format!("Insertion failed: {e}"));
                return format!("error: {e}");
            }
        };
        if !pasted {
            // The overlay is deliberately quiet; the notification is the
            // cross-desktop "you can paste now" signal.
            notify("FlowOSS ✓ copied — press Ctrl+V", &text);
        }
        self.emit(StateEvent::Success {
            text: text.clone(),
            pasted,
        });
        self.hide_at = Some(Instant::now() + LINGER);
        text
    }

    fn insert(&mut self, text: &str) -> anyhow::Result<InsertOutcome> {
        let mode = self.settings.paste_mode();
        match self.inserter.as_mut() {
            Some(inserter) => inserter.insert(text, mode),
            None => {
                self.inserter = Some(Inserter::new()?);
                self.inserter.as_mut().unwrap().insert(text, mode)
            }
        }
    }

    fn cancel(&mut self) -> String {
        if self.recording.take().is_some() {
            self.overlay_visible(false);
            self.emit(StateEvent::Idle);
            "cancelled".into()
        } else {
            "idle".into()
        }
    }

    fn copy_last(&mut self) -> String {
        if self.last_transcript.is_empty() {
            return "no transcript yet".into();
        }
        let text = self.last_transcript.clone();
        match self.inserter.as_mut() {
            Some(inserter) => match inserter.insert(&text, flowoss_insertion::PasteMode::Copy) {
                Ok(_) => {
                    notify("FlowOSS ✓ copied — press Ctrl+V", &text);
                    text
                }
                Err(e) => format!("error: {e}"),
            },
            None => "error: clipboard unavailable".into(),
        }
    }

    fn show_error(&mut self, message: &str) {
        self.overlay_visible(true);
        self.emit(StateEvent::Error {
            message: message.into(),
        });
        self.hide_at = Some(Instant::now() + LINGER * 2);
        notify("FlowOSS error", message);
    }
}
