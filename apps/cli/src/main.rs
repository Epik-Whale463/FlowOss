//! FlowOSS CLI — milestones M0/M1 of the PRD.
//!
//! `flowoss transcribe <wav>`  proves local Parakeet transcription (M0).
//! `flowoss listen`            records the mic, runs VAD, transcribes (M1).

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use flowoss_text_cleanup::CleanupMode;

mod daemon;

#[derive(Parser)]
#[command(name = "flowoss", version, about = "Local-first voice dictation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List available microphones (default device first)
    Devices,
    /// Transcribe a WAV file with the local Parakeet model
    Transcribe {
        /// Path to a WAV file
        file: PathBuf,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Record from the microphone, detect speech, and print the transcript
    Listen {
        /// Input device name (see `flowoss devices`); default device if omitted
        #[arg(short, long)]
        device: Option<String>,
        /// Stop automatically after this many seconds (default: stop on Enter)
        #[arg(short, long)]
        seconds: Option<f32>,
        /// Skip voice activity detection
        #[arg(long)]
        no_vad: bool,
        /// Cleanup mode: raw | basic
        #[arg(long, default_value = "basic")]
        cleanup: CleanupMode,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Run the dictation daemon: keeps the model warm and waits for triggers
    Daemon {
        /// Input device name; default device if omitted
        #[arg(short, long)]
        device: Option<String>,
        /// Insertion mode: auto (clipboard + simulated paste) | copy
        #[arg(long, default_value = "auto")]
        paste_mode: flowoss_insertion::PasteMode,
        /// Cleanup mode: raw | basic
        #[arg(long, default_value = "basic")]
        cleanup: CleanupMode,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Toggle recording on the running daemon (bind this to a hotkey)
    Trigger,
    /// Toggle assist mode on the desktop app: selected text + spoken question
    Assist,
    /// Cancel an in-progress recording
    Cancel,
    /// Print the last transcript
    Last {
        /// Also copy it to the clipboard
        #[arg(long)]
        copy: bool,
    },
    /// Stop the running daemon
    Quit,
}

#[derive(clap::Args)]
struct ModelArgs {
    /// Directory containing the Parakeet ONNX model
    #[arg(long, default_value_os_t = flowoss_core::default_stt_model_dir())]
    model_dir: PathBuf,
    /// Path to the Silero VAD model
    #[arg(long, default_value_os_t = flowoss_core::default_vad_model_path())]
    vad_model: PathBuf,
    /// Inference threads for the STT model
    #[arg(long, default_value_t = 4)]
    threads: i32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Devices => {
            for (i, name) in flowoss_audio::list_input_devices()?.iter().enumerate() {
                let marker = if i == 0 { " (default)" } else { "" };
                println!("{name}{marker}");
            }
        }
        Command::Transcribe { file, model } => {
            let samples = flowoss_audio::load_wav(&file)?;
            let mut stt = load_stt(&model)?;
            let start = Instant::now();
            let text = stt.transcribe(&samples);
            eprintln!(
                "[{:.2}s audio transcribed in {:.2}s]",
                samples.len() as f32 / flowoss_core::SAMPLE_RATE as f32,
                start.elapsed().as_secs_f32()
            );
            println!("{text}");
        }
        Command::Listen {
            device,
            seconds,
            no_vad,
            cleanup,
            model,
        } => {
            // Load models before recording so the mic isn't running while
            // the (slow) first model load happens.
            let mut stt = load_stt(&model)?;
            let mut vad = if no_vad {
                None
            } else {
                Some(flowoss_vad::SpeechDetector::new(&model.vad_model)?)
            };

            let recording = flowoss_audio::Recording::start(device.as_deref())?;
            match seconds {
                Some(secs) => {
                    eprintln!("Recording for {secs:.1}s...");
                    let start = Instant::now();
                    while start.elapsed().as_secs_f32() < secs {
                        show_level(&recording);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                None => {
                    eprintln!("Recording... press Enter to stop.");
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                }
            }
            let samples = recording.stop();
            eprint!("\r");
            eprintln!("Captured {:.2}s of audio.", samples.len() as f32 / 16000.0);

            let speech = match vad.as_mut() {
                Some(vad) => match vad.extract_speech(&samples) {
                    Some(speech) => speech,
                    None => bail!("no speech detected"),
                },
                None => samples,
            };

            let start = Instant::now();
            let text = stt.transcribe(&speech);
            eprintln!("[transcribed in {:.2}s]", start.elapsed().as_secs_f32());
            if text.is_empty() {
                bail!("empty transcript");
            }
            println!("{}", flowoss_text_cleanup::clean(&text, cleanup));
        }
        Command::Daemon {
            device,
            paste_mode,
            cleanup,
            model,
        } => {
            let stt = load_stt(&model)?;
            let vad = flowoss_vad::SpeechDetector::new(&model.vad_model)?;
            daemon::run(
                stt,
                vad,
                daemon::DaemonOptions {
                    device,
                    paste_mode,
                    cleanup,
                },
            )?;
        }
        Command::Trigger => println!("{}", daemon::send_command("toggle")?),
        Command::Assist => println!("{}", daemon::send_command("assist")?),
        Command::Cancel => println!("{}", daemon::send_command("cancel")?),
        Command::Last { copy } => {
            let command = if copy { "copy-last" } else { "last" };
            println!("{}", daemon::send_command(command)?);
        }
        Command::Quit => println!("{}", daemon::send_command("quit")?),
    }
    Ok(())
}

fn load_stt(model: &ModelArgs) -> Result<flowoss_stt::Transcriber> {
    if !model.model_dir.exists() {
        bail!(
            "model directory not found: {}\nRun ./scripts/download-models.sh first, \
             or pass --model-dir.",
            model.model_dir.display()
        );
    }
    eprintln!("Loading model from {} ...", model.model_dir.display());
    let start = Instant::now();
    let stt = flowoss_stt::Transcriber::from_model_dir(&model.model_dir, model.threads)?;
    eprintln!("Model loaded in {:.2}s.", start.elapsed().as_secs_f32());
    Ok(stt)
}

fn show_level(recording: &flowoss_audio::Recording) {
    let level = recording.level();
    let bars = (level * 200.0).min(30.0) as usize;
    eprint!("\r[{:<30}] {:5.1}s", "#".repeat(bars), recording.duration_secs());
    let _ = std::io::stderr().flush();
}
