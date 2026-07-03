# FlowOSS

FlowOSS is a local-first, open-source voice dictation app for Linux and Windows.
Press a shortcut, speak naturally, release, and your text is transcribed on-device and inserted into the active app. No cloud transcription, no telemetry, no always-online dependency.

It is built in Rust around `sherpa-onnx`, NVIDIA Parakeet speech models, Silero VAD, and a small desktop shell for tray, overlay, and settings UX.

## MVP Status

The current MVP is Linux-first and working on Ubuntu GNOME Wayland.

Available today:

- Local microphone capture
- Voice activity detection
- On-device speech-to-text with Parakeet models
- Global shortcut driven dictation flow
- Clipboard-based insertion with auto-paste support
- Tray app, overlay, and settings window
- Last transcript recovery
- Experimental assist mode for highlighted-text Q&A

Current focus areas after MVP:

- Windows polish
- Packaging and installer flow
- More robust insertion across desktop environments
- Better setup UX for first-time users

## Why FlowOSS

- Private: transcription runs locally
- Open: Rust workspace with modular crates
- Practical: works across apps instead of inside a single editor
- Hackable: settings, models, insertion behavior, and future assist workflows are all extensible

## Workspace Layout

```text
apps/cli/              CLI binary (`flowoss`)
apps/desktop/          Desktop shell (tray, overlay, settings)
crates/core/           shared constants, paths, app state helpers
crates/audio/          microphone capture and WAV loading
crates/vad/            Silero VAD integration
crates/stt/            Parakeet transcription via sherpa-onnx
crates/text_cleanup/   transcript cleanup modes
crates/insertion/      clipboard, paste simulation, notifications
crates/assist/         provider-backed highlighted-text assist mode
scripts/               setup helpers such as model download
docs/                  product spec and supporting notes
packaging/             service and packaging assets
```

## Quick Start

### 1. Install prerequisites

Linux prerequisites:

- Rust toolchain via `rustup`
- ALSA development headers: `sudo apt install libasound2-dev`

Depending on desktop integration choices, you may also want tools such as `wl-clipboard`, `xdotool`, or equivalent platform packages already available on your system.

### 2. Download speech models

```bash
./scripts/download-models.sh
```

This downloads the default Parakeet and VAD assets into `models/`.

### 3. Build

```bash
cargo build --release
```

### 4. Try the CLI

```bash
# List microphones
./target/release/flowoss devices

# Transcribe a WAV file
./target/release/flowoss transcribe recording.wav

# Live dictation to stdout
./target/release/flowoss listen
```

## Dictation Daemon Flow

The daemon keeps the speech stack warm and handles shortcut-triggered dictation:

```bash
./target/release/flowoss daemon &
./target/release/flowoss trigger
./target/release/flowoss trigger
./target/release/flowoss cancel
./target/release/flowoss last --copy
./target/release/flowoss quit
```

Typical flow:

1. Focus any text field.
2. Trigger dictation.
3. Speak naturally.
4. Trigger again to stop.
5. FlowOSS transcribes locally and inserts or copies the result.

## Desktop App

The desktop shell adds:

- tray icon controls
- non-focus-stealing overlay status UI
- settings window for microphone, model path, paste mode, cleanup, and shortcuts
- assist shortcut support

Build artifacts for the desktop target come from the same Rust workspace.

## Assist Mode

Assist mode is an experimental MVP-adjacent feature.
Highlight text in any app, trigger the assist shortcut, ask a spoken question, and FlowOSS sends the selected text plus your transcribed question to the configured provider, then shows the answer in the overlay.

Supported provider styles:

- Ollama
- Anthropic-compatible APIs
- OpenAI-compatible APIs

Speech-to-text remains local. Provider calls only happen if you explicitly configure and use assist mode.

## Linux Shortcut Example

On GNOME, you can bind the CLI trigger to a custom shortcut:

```bash
KB=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/flowoss/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$KB']"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB name 'FlowOSS dictation toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB command "$PWD/target/release/flowoss trigger"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB binding '<Super>z'
```

## Product Spec

The original MVP definition and roadmap live in [`docs/flowoss-prd.md`](docs/flowoss-prd.md).

## Model And Dependency Licensing

- FlowOSS source code: `MIT OR Apache-2.0`
- NVIDIA Parakeet models: CC-BY-4.0
- `sherpa-onnx`: Apache-2.0
- Silero VAD: see upstream project licensing

Models are not bundled into the repository and are only downloaded on explicit user action.

## Repository Notes

- `models/` is intentionally ignored except for a placeholder file
- `target/` build artifacts are ignored
- GitHub remote is configured as the main project origin

## License

Licensed under either of:

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
