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
    AssistToggle(Option<Sender<String>>),
    Cancel(Option<Sender<String>>),
    Last(Sender<String>),
    CopyLast(Option<Sender<String>>),
    UpdateSettings(Settings),
    /// A background assist worker finished; `gen` guards against stale results.
    AssistDone {
        gen: u64,
        result: Result<flowoss_assist::Answer, String>,
        query: String,
        context_preview: String,
    },
    /// Mouse entered/left the interactive answer card (pins/unpins auto-hide).
    AssistHover(bool),
    /// Close button on the answer card.
    DismissOverlay,
    /// Copy arbitrary text (the answer) to the clipboard.
    CopyText(String, Option<Sender<String>>),
}

/// Event payload for the overlay and settings UIs.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum StateEvent {
    Loading,
    Idle,
    Recording { level: f32, secs: f32, partial: String },
    AssistRecording {
        level: f32,
        secs: f32,
        partial: String,
        context_preview: String,
    },
    Processing,
    AssistProcessing { query: String, status: String },
    Success { text: String, pasted: bool },
    AssistAnswer {
        answer: String,
        query: String,
        context_preview: String,
        sources: Vec<flowoss_assist::Source>,
    },
    NoSpeech,
    Error { message: String },
}

const STATE_EVENT: &str = "flowoss://state";
/// How long success/error stays on screen before the overlay hides.
const LINGER: Duration = Duration::from_millis(1600);
/// Answers linger longer, and hovering the card pins them open.
const ANSWER_LINGER: Duration = Duration::from_secs(30);
/// Grace period after the pointer leaves a pinned answer.
const HOVER_LINGER: Duration = Duration::from_secs(8);
const PILL_SIZE: (u32, u32) = (430, 64);
const ANSWER_SIZE: (u32, u32) = (640, 340);

/// Place the overlay just above the bottom edge, horizontally centered on
/// the monitor the cursor's window is on (or the primary one).
fn position_bottom_center(window: &tauri::WebviewWindow) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) else {
        return;
    };
    let margin = (56.0 * monitor.scale_factor()) as i32;
    let x = monitor.position().x + (monitor.size().width as i32 - size.width as i32) / 2;
    let y = monitor.position().y + monitor.size().height as i32 - size.height as i32 - margin;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn spawn(app: AppHandle, settings: Settings) -> Sender<Command> {
    let (tx, rx) = channel();
    let engine_tx = tx.clone();
    std::thread::Builder::new()
        .name("dictation-engine".into())
        .spawn(move || run(app, settings, engine_tx, rx))
        .expect("failed to spawn engine thread");
    tx
}

struct Engine {
    app: AppHandle,
    /// Loop-back sender so background assist workers can report results.
    tx: Sender<Command>,
    settings: Settings,
    stt: Option<flowoss_stt::Transcriber>,
    vad: Option<flowoss_vad::SpeechDetector>,
    streaming: Option<flowoss_stt::StreamingTranscriber>,
    inserter: Option<Inserter>,
    recording: Option<flowoss_audio::Recording>,
    assist_context: Option<String>,
    /// Bumped whenever an in-flight assist result should be discarded.
    assist_gen: u64,
    /// True while the interactive answer card is on screen.
    answer_showing: bool,
    stream_cursor: usize,
    partial: String,
    last_transcript: String,
    hide_at: Option<Instant>,
    click_through: Option<bool>,
}

fn run(app: AppHandle, settings: Settings, tx: Sender<Command>, rx: Receiver<Command>) {
    let mut engine = Engine {
        app,
        tx,
        settings,
        stt: None,
        vad: None,
        streaming: None,
        inserter: None,
        recording: None,
        assist_context: None,
        assist_gen: 0,
        answer_showing: false,
        stream_cursor: 0,
        partial: String::new(),
        last_transcript: std::fs::read_to_string(flowoss_core::last_transcript_path())
            .unwrap_or_default(),
        hide_at: None,
        click_through: None,
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

    fn overlay_visible(&mut self, visible: bool) {
        self.overlay_visible_sized(visible, PILL_SIZE, false);
    }

    fn overlay_answer_visible(&mut self, interactive: bool) {
        self.overlay_visible_sized(true, ANSWER_SIZE, interactive);
    }

    fn overlay_visible_sized(&mut self, visible: bool, size: (u32, u32), interactive: bool) {
        match self.app.get_webview_window("overlay") {
            Some(window) => {
                if visible {
                    let _ = window.set_size(tauri::PhysicalSize::new(size.0, size.1));
                    position_bottom_center(&window);
                }
                let result = if visible { window.show() } else { window.hide() };
                if let Err(e) = result {
                    eprintln!("overlay {}: {e}", if visible { "show" } else { "hide" });
                } else if visible && self.click_through != Some(!interactive) {
                    // The pill is click-through; the answer card accepts the
                    // mouse (scroll, copy, sources). Safe only after the
                    // window has been realized by show().
                    let _ = window.set_ignore_cursor_events(!interactive);
                    self.click_through = Some(!interactive);
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
        // Live preview is best-effort: no streaming model, no live words.
        self.streaming = if self.settings.live_preview {
            match flowoss_stt::StreamingTranscriber::from_model_dir(
                &self.settings.streaming_model_dir,
                2,
            ) {
                Ok(streaming) => Some(streaming),
                Err(e) => {
                    eprintln!("streaming preview unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };
        if self.inserter.is_none() {
            self.inserter = Inserter::new().ok();
        }
    }

    fn tick(&mut self) {
        if let Some(recording) = &self.recording {
            if let Some(streaming) = self.streaming.as_mut() {
                let chunk = recording.drain_new(&mut self.stream_cursor);
                let text = streaming.feed(&chunk);
                if !text.is_empty() {
                    self.partial = text;
                }
            }
            if let Some(context) = &self.assist_context {
                self.emit(StateEvent::AssistRecording {
                    level: recording.level(),
                    secs: recording.duration_secs(),
                    partial: self.partial.clone(),
                    context_preview: preview(context),
                });
            } else {
                self.emit(StateEvent::Recording {
                    level: recording.level(),
                    secs: recording.duration_secs(),
                    partial: self.partial.clone(),
                });
            }
        } else if let Some(hide_at) = self.hide_at {
            if Instant::now() >= hide_at {
                self.hide_at = None;
                self.answer_showing = false;
                self.overlay_visible(false);
                self.emit(StateEvent::Idle);
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Toggle(reply) => self.toggle(reply),
            Command::AssistToggle(reply) => self.assist_toggle(reply),
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
            Command::AssistDone {
                gen,
                result,
                query,
                context_preview,
            } => self.assist_done(gen, result, query, context_preview),
            Command::AssistHover(hovering) => {
                if self.answer_showing {
                    // Hovering pins the answer open; leaving restarts a
                    // short countdown so it doesn't overstay.
                    self.hide_at = if hovering {
                        None
                    } else {
                        Some(Instant::now() + HOVER_LINGER)
                    };
                }
            }
            Command::DismissOverlay => {
                self.assist_gen += 1; // drop any in-flight answer too
                self.answer_showing = false;
                self.hide_at = None;
                self.overlay_visible(false);
                self.emit(StateEvent::Idle);
            }
            Command::CopyText(text, reply) => {
                let result = self.copy_text(&text);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
        }
    }

    fn reply_to(reply: Option<Sender<String>>, message: String) {
        if let Some(reply) = reply {
            let _ = reply.send(message);
        }
    }

    fn toggle(&mut self, reply: Option<Sender<String>>) {
        match self.recording.take() {
            None => {
                let result = self.start_recording();
                Self::reply_to(reply, result);
            }
            Some(recording) if self.assist_context.is_some() => {
                let context = self.assist_context.take().unwrap();
                self.finish_assist_recording(recording, context, reply);
            }
            Some(recording) => {
                let result = self.finish_recording(recording);
                Self::reply_to(reply, result);
            }
        }
    }

    fn assist_toggle(&mut self, reply: Option<Sender<String>>) {
        match self.recording.take() {
            None => {
                let result = self.start_assist_recording();
                Self::reply_to(reply, result);
            }
            Some(recording) if self.assist_context.is_some() => {
                let context = self.assist_context.take().unwrap();
                self.finish_assist_recording(recording, context, reply);
            }
            Some(recording) => {
                let result = self.finish_recording(recording);
                Self::reply_to(reply, result);
            }
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
                self.answer_showing = false;
                self.stream_cursor = 0;
                self.partial.clear();
                if let Some(streaming) = self.streaming.as_mut() {
                    streaming.reset();
                }
                self.overlay_visible(true);
                self.emit(StateEvent::Recording {
                    level: 0.0,
                    secs: 0.0,
                    partial: String::new(),
                });
                "recording".into()
            }
            Err(e) => {
                let message = format!("Microphone error: {e}");
                self.show_error(&message);
                format!("error: {message}")
            }
        }
    }

    fn start_assist_recording(&mut self) -> String {
        if self.stt.is_none() {
            let message = format!(
                "Model not loaded — check model directory {}",
                self.settings.model_dir.display()
            );
            self.show_error(&message);
            return format!("error: {message}");
        }

        let context = match self.capture_selection() {
            Ok(context) => context,
            Err(e) => {
                let message = e.to_string();
                self.show_error(&message);
                return format!("error: {message}");
            }
        };

        match flowoss_audio::Recording::start(self.settings.device.as_deref()) {
            Ok(recording) => {
                self.recording = Some(recording);
                self.assist_context = Some(context.clone());
                self.hide_at = None;
                self.answer_showing = false;
                self.stream_cursor = 0;
                self.partial.clear();
                if let Some(streaming) = self.streaming.as_mut() {
                    streaming.reset();
                }
                self.overlay_visible(true);
                self.emit(StateEvent::AssistRecording {
                    level: 0.0,
                    secs: 0.0,
                    partial: String::new(),
                    context_preview: preview(&context),
                });
                "assist recording".into()
            }
            Err(e) => {
                self.assist_context = None;
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

    fn finish_assist_recording(
        &mut self,
        recording: flowoss_audio::Recording,
        context: String,
        reply: Option<Sender<String>>,
    ) {
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
            Self::reply_to(reply, "no speech".into());
            return;
        };

        let Some(stt) = self.stt.as_mut() else {
            self.show_error("Model not loaded");
            Self::reply_to(reply, "error: model not loaded".into());
            return;
        };
        let query = stt.transcribe(&speech);
        if query.is_empty() {
            self.emit(StateEvent::NoSpeech);
            self.hide_at = Some(Instant::now() + LINGER);
            Self::reply_to(reply, "no speech".into());
            return;
        }
        let query = flowoss_text_cleanup::clean(&query, self.settings.cleanup_mode());
        self.overlay_answer_visible(false);
        self.hide_at = None;
        self.emit(StateEvent::AssistProcessing {
            query: query.clone(),
            status: "Thinking…".into(),
        });

        // The LLM (and its web searches) can take a while; run it off the
        // engine thread so dictation stays responsive, and report back via
        // AssistDone. `gen` invalidates the result if the user cancels.
        self.assist_gen += 1;
        let gen = self.assist_gen;
        let app = self.app.clone();
        let tx = self.tx.clone();
        let config = self.settings.assist_config();
        let context_preview = preview(&context);
        std::thread::Builder::new()
            .name("assist-worker".into())
            .spawn(move || {
                let progress_query = query.clone();
                let progress_app = app.clone();
                let progress = move |status: &str| {
                    let _ = progress_app.emit(
                        STATE_EVENT,
                        &StateEvent::AssistProcessing {
                            query: progress_query.clone(),
                            status: status.into(),
                        },
                    );
                };
                let result = flowoss_assist::ask(&config, &context, &query, &progress)
                    .map_err(|e| e.to_string());
                let message = match &result {
                    Ok(answer) => answer.text.clone(),
                    Err(e) => format!("error: {e}"),
                };
                Self::reply_to(reply, message);
                let _ = tx.send(Command::AssistDone {
                    gen,
                    result,
                    query,
                    context_preview,
                });
            })
            .expect("failed to spawn assist worker");
    }

    fn assist_done(
        &mut self,
        gen: u64,
        result: Result<flowoss_assist::Answer, String>,
        query: String,
        context_preview: String,
    ) {
        if gen != self.assist_gen {
            return; // cancelled or superseded while the worker ran
        }
        if self.recording.is_some() {
            return; // the user has moved on to a new dictation
        }
        match result {
            Ok(answer) => {
                self.answer_showing = true;
                self.overlay_answer_visible(true);
                self.emit(StateEvent::AssistAnswer {
                    answer: answer.text,
                    query,
                    context_preview,
                    sources: answer.sources,
                });
                self.hide_at = Some(Instant::now() + ANSWER_LINGER);
            }
            Err(e) => self.show_error(&format!("Assistant failed: {e}")),
        }
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

    fn capture_selection(&mut self) -> anyhow::Result<String> {
        match self.inserter.as_mut() {
            Some(inserter) => inserter.capture_selection(),
            None => {
                self.inserter = Some(Inserter::new()?);
                self.inserter.as_mut().unwrap().capture_selection()
            }
        }
    }

    fn cancel(&mut self) -> String {
        self.assist_gen += 1; // discard any in-flight assist answer
        if self.recording.take().is_some() || self.answer_showing {
            self.assist_context = None;
            self.answer_showing = false;
            self.hide_at = None;
            self.overlay_visible(false);
            self.emit(StateEvent::Idle);
            "cancelled".into()
        } else {
            "idle".into()
        }
    }

    fn copy_text(&mut self, text: &str) -> String {
        if text.is_empty() {
            return "nothing to copy".into();
        }
        match self.inserter.as_mut() {
            Some(inserter) => match inserter.insert(text, flowoss_insertion::PasteMode::Copy) {
                Ok(_) => "copied".into(),
                Err(e) => format!("error: {e}"),
            },
            None => "error: clipboard unavailable".into(),
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

fn preview(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = compact.chars().take(180).collect();
    if compact.chars().count() > 180 {
        out.push_str("...");
    }
    out
}
