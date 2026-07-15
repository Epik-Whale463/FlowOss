//! Dictation engine: a dedicated thread that owns the microphone stream,
//! the warm STT/VAD models, and the clipboard.
//!
//! The cpal recording handle is not `Send`, so everything audio-related
//! lives here; the rest of the app talks to it through a channel. While
//! recording, the loop ticks every 80 ms to stream live mic levels to the
//! overlay.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use flowoss_audio::FeedbackCue;
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
    /// Audition the sound family from Settings.
    PreviewSound,
}

/// Event payload for the overlay and settings UIs.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum StateEvent {
    Loading,
    /// First-run model download progress.
    Downloading { message: String, pct: u32 },
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
/// Keep speech models in memory briefly after use so repeated dictation is fast.
const MODEL_KEEP_WARM: Duration = Duration::from_secs(10 * 60);
/// Compact 3D status capsule. Tall enough for cylindrical lighting to read,
/// wide enough for a live waveform; still a transient click-through layer.
const PILL_SIZE: (u32, u32) = (54, 24);
/// Compact thinking card (status + query); grows to ANSWER_SIZE on reply.
const THINKING_SIZE: (u32, u32) = (420, 88);
const ANSWER_SIZE: (u32, u32) = (560, 340);

/// Place the overlay just below the top edge, horizontally centered on
/// the monitor the cursor's window is on (or the primary one) — a notch
/// light, out of the way of what the user is typing.
fn position_top_center(window: &tauri::WebviewWindow, size: (u32, u32)) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let margin = (14.0 * monitor.scale_factor()) as i32;
    let x = monitor.position().x + (monitor.size().width as i32 - size.0 as i32) / 2;
    let y = monitor.position().y + margin;
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
    inserter: Option<Inserter>,
    recording: Option<flowoss_audio::Recording>,
    assist_context: Option<String>,
    /// Bumped whenever an in-flight assist result should be discarded.
    assist_gen: u64,
    /// True while the interactive answer card is on screen.
    answer_showing: bool,
    last_transcript: String,
    hide_at: Option<Instant>,
    models_unload_at: Option<Instant>,
    click_through: Option<bool>,
    feedback: Option<flowoss_audio::FeedbackPlayer>,
    feedback_unavailable: bool,
}

fn run(app: AppHandle, settings: Settings, tx: Sender<Command>, rx: Receiver<Command>) {
    let mut engine = Engine {
        app,
        tx,
        settings,
        stt: None,
        vad: None,
        inserter: None,
        recording: None,
        assist_context: None,
        assist_gen: 0,
        answer_showing: false,
        last_transcript: std::fs::read_to_string(flowoss_core::last_transcript_path())
            .unwrap_or_default(),
        hide_at: None,
        models_unload_at: None,
        click_through: None,
        feedback: None,
        feedback_unavailable: false,
    };
    engine.ensure_models_ready();
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
                    // GTK ignores resize() on non-resizable windows, so the
                    // overlay snaps back to its natural ~200x200 size (a
                    // square pill renders as a circle). Min/max geometry
                    // hints are enforced even for non-resizable windows, so
                    // pin them to the target size on every resize.
                    let physical = tauri::PhysicalSize::new(size.0, size.1);
                    let _ = window.set_min_size(Some(physical));
                    let _ = window.set_max_size(Some(physical));
                    let _ = window.set_size(physical);
                    position_top_center(&window, size);
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

    /// On first run, download the speech models before trying to load them,
    /// keeping the overlay updated with progress.
    fn ensure_models_ready(&mut self) {
        if crate::download::models_present(&self.settings) {
            return;
        }
        self.overlay_visible(true);
        self.emit(StateEvent::Downloading {
            message: "Preparing first-run download…".into(),
            pct: 0,
        });
        let app = self.app.clone();
        let result = crate::download::ensure_models(&self.settings, |label, pct| {
            let _ = app.emit(
                STATE_EVENT,
                &StateEvent::Downloading {
                    message: label.to_string(),
                    pct,
                },
            );
        });
        if let Err(e) = result {
            self.show_error(&format!("Model download failed: {e}"));
        }
    }

    fn ensure_models_loaded(&mut self) -> Result<(), String> {
        if self.stt.is_some() {
            self.touch_models();
            return Ok(());
        }

        self.overlay_visible(true);
        self.emit(StateEvent::Loading);
        self.load_models()?;
        self.emit(StateEvent::Idle);
        Ok(())
    }

    fn load_models(&mut self) -> Result<(), String> {
        self.unload_models();
        let stt = flowoss_stt::Transcriber::from_model_dir(
            &self.settings.model_dir,
            self.settings.threads,
        )
        .map_err(|e| format!("STT load failed: {e}"))?;
        let vad = match flowoss_vad::SpeechDetector::new(&self.settings.vad_model) {
            Ok(vad) => Some(vad),
            Err(e) => {
                eprintln!("VAD load failed: {e}");
                None
            }
        };
        self.stt = Some(stt);
        self.vad = vad;
        self.touch_models();
        if self.inserter.is_none() {
            self.inserter = Inserter::new().ok();
        }
        Ok(())
    }

    fn touch_models(&mut self) {
        self.models_unload_at = Some(Instant::now() + MODEL_KEEP_WARM);
    }

    fn unload_models(&mut self) {
        self.stt = None;
        self.vad = None;
        self.models_unload_at = None;
    }

    fn tick(&mut self) {
        if let Some(recording) = &self.recording {
            if let Some(context) = &self.assist_context {
                self.emit(StateEvent::AssistRecording {
                    level: recording.level(),
                    secs: recording.duration_secs(),
                    partial: String::new(),
                    context_preview: preview(context),
                });
            } else {
                self.emit(StateEvent::Recording {
                    level: recording.level(),
                    secs: recording.duration_secs(),
                    partial: String::new(),
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
        if self.recording.is_none()
            && self.models_unload_at.is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.unload_models();
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
                    self.unload_models();
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
            Command::PreviewSound => self.play_feedback(FeedbackCue::Success),
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
        if let Err(e) = self.ensure_models_loaded() {
            let message = format!("Model not loaded — {e}");
            self.show_error(&message);
            return format!("error: {message}");
        }
        match flowoss_audio::Recording::start(self.settings.device.as_deref()) {
            Ok(recording) => {
                self.recording = Some(recording);
                self.hide_at = None;
                self.answer_showing = false;
                self.overlay_visible(true);
                self.emit(StateEvent::Recording {
                    level: 0.0,
                    secs: 0.0,
                    partial: String::new(),
                });
                self.play_feedback(FeedbackCue::RecordStart);
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
        if let Err(e) = self.ensure_models_loaded() {
            let message = format!("Model not loaded — {e}");
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
                self.overlay_visible(true);
                self.emit(StateEvent::AssistRecording {
                    level: 0.0,
                    secs: 0.0,
                    partial: String::new(),
                    context_preview: preview(&context),
                });
                self.play_feedback(FeedbackCue::AssistStart);
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
        self.play_feedback(FeedbackCue::RecordStop);
        self.touch_models();
        self.emit(StateEvent::Processing);
        self.overlay_visible(true);

        let speech = match self.vad.as_mut() {
            Some(vad) => vad.extract_speech(&samples),
            None => Some(samples),
        };
        let Some(speech) = speech else {
            self.emit(StateEvent::NoSpeech);
            self.play_feedback(FeedbackCue::NoSpeech);
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
            self.play_feedback(FeedbackCue::NoSpeech);
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
        self.play_feedback(FeedbackCue::Success);
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
        self.play_feedback(FeedbackCue::RecordStop);
        self.touch_models();
        self.emit(StateEvent::Processing);
        self.overlay_visible(true);

        let speech = match self.vad.as_mut() {
            Some(vad) => vad.extract_speech(&samples),
            None => Some(samples),
        };
        let Some(speech) = speech else {
            self.emit(StateEvent::NoSpeech);
            self.play_feedback(FeedbackCue::NoSpeech);
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
            self.play_feedback(FeedbackCue::NoSpeech);
            self.hide_at = Some(Instant::now() + LINGER);
            Self::reply_to(reply, "no speech".into());
            return;
        }
        let query = flowoss_text_cleanup::clean(&query, self.settings.cleanup_mode());
        self.overlay_visible_sized(true, THINKING_SIZE, false);
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
                self.play_feedback(FeedbackCue::AssistAnswer);
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
            self.play_feedback(FeedbackCue::Cancel);
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
        self.play_feedback(FeedbackCue::Error);
        self.hide_at = Some(Instant::now() + LINGER * 2);
        notify("FlowOSS error", message);
    }

    fn play_feedback(&mut self, cue: FeedbackCue) {
        if !self.settings.feedback_sounds || self.feedback_unavailable {
            return;
        }
        if self.feedback.is_none() {
            match flowoss_audio::FeedbackPlayer::new() {
                Ok(player) => self.feedback = Some(player),
                Err(e) => {
                    // Feedback is deliberately supplemental; never disrupt
                    // dictation just because a machine has no audio output.
                    eprintln!("feedback sounds unavailable: {e}");
                    self.feedback_unavailable = true;
                    return;
                }
            }
        }
        if let Some(player) = &self.feedback {
            player.play(cue, self.settings.feedback_volume);
        }
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
