# FlowOSS

Local-first, open-source voice dictation for Linux and Windows. Hold a key, speak naturally, release, text appears — with all speech-to-text running on your machine via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) and NVIDIA Parakeet models. No cloud, no telemetry.

See [docs/flowoss-prd.md](docs/flowoss-prd.md) for the full product spec.

## Status

Early development. Milestones M0–M2 working on Ubuntu GNOME Wayland:
hotkey toggle → record → VAD → local transcription → clipboard (+ optional
simulated paste via `ydotool` if installed).

## Prerequisites (Linux)

- Rust toolchain (`rustup`)
- ALSA headers: `sudo apt install libasound2-dev`

## Quick start

```bash
# Download models (~700MB Parakeet TDT 0.6b v2 int8 + Silero VAD)
./scripts/download-models.sh

cargo build --release

# List microphones
./target/release/flowoss devices

# Transcribe a WAV file (M0 proof)
./target/release/flowoss transcribe recording.wav

# Live dictation to stdout: speak, press Enter to stop (M1)
./target/release/flowoss listen
```

## Dictation daemon (M2)

The daemon keeps the model warm and waits for hotkey triggers:

```bash
./target/release/flowoss daemon &        # start once per session
./target/release/flowoss trigger         # 1st press: start recording
./target/release/flowoss trigger         # 2nd press: transcribe + copy/paste
./target/release/flowoss cancel          # abort a recording
./target/release/flowoss last [--copy]   # recover last transcript
./target/release/flowoss quit            # stop the daemon
```

Bind `flowoss trigger` to a keyboard shortcut. On GNOME:

```bash
KB=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/flowoss/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$KB']"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB name 'FlowOSS dictation toggle'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB command "$PWD/target/release/flowoss trigger"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB binding '<Super>z'
```

Flow: focus any text field → Super+Z → speak → Super+Z → the transcript is
in your clipboard (a notification confirms) → Ctrl+V. If `ydotool` is set
up, the paste happens automatically (`--paste-mode auto`, the default).

## Workspace layout

```
apps/cli/              CLI prototype (binary: flowoss)
crates/core/           shared constants and paths
crates/audio/          mic capture (cpal), resampling, WAV loading
crates/vad/            Silero voice activity detection
crates/stt/            Parakeet transcription via sherpa-onnx
crates/text_cleanup/   raw/basic transcript cleanup
crates/insertion/      clipboard + paste simulation, notifications
scripts/               model download helper
docs/                  PRD
```

## Model licenses

- NVIDIA Parakeet models: CC-BY-4.0 — attribution to NVIDIA required.
- sherpa-onnx: Apache-2.0.
- Silero VAD: see [upstream license](https://github.com/snakers4/silero-vad).

Models are downloaded on explicit user action only; nothing is bundled.
