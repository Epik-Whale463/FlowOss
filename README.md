# FlowOSS

Local-first, open-source voice dictation for Linux and Windows. Hold a key, speak naturally, release, text appears — with all speech-to-text running on your machine via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) and NVIDIA Parakeet models. No cloud, no telemetry.

See [docs/flowoss-prd.md](docs/flowoss-prd.md) for the full product spec.

## Status

Early development. Current milestone: **M1 — CLI prototype** (record mic → VAD → local transcription → print text).

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

## Workspace layout

```
apps/cli/              CLI prototype (binary: flowoss)
crates/core/           shared constants and paths
crates/audio/          mic capture (cpal), resampling, WAV loading
crates/vad/            Silero voice activity detection
crates/stt/            Parakeet transcription via sherpa-onnx
crates/text_cleanup/   raw/basic transcript cleanup
scripts/               model download helper
docs/                  PRD
```

## Model licenses

- NVIDIA Parakeet models: CC-BY-4.0 — attribution to NVIDIA required.
- sherpa-onnx: Apache-2.0.
- Silero VAD: see [upstream license](https://github.com/snakers4/silero-vad).

Models are downloaded on explicit user action only; nothing is bundled.
