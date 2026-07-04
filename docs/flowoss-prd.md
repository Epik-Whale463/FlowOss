# PRD: Open-Source Local Voice Dictation App

Working name: **FlowOSS**

Goal: Build a local-first, open-source Wispr Flow-style dictation app for **Ubuntu Linux** and **Windows**, using high-quality local STT models like NVIDIA Parakeet through `sherpa-onnx`.

## 1. Summary

FlowOSS is a desktop app that lets users dictate into any text field across the OS using a global shortcut. The app records microphone audio, transcribes locally, optionally cleans up the result, and inserts the final text into the active app.

The first target environments are:

| Platform | Target |
|---|---|
| Linux | Ubuntu 26.04, GNOME, Wayland |
| Windows | Windows 10/11 |
| STT | Local ONNX models |
| Default model | Parakeet TDT/Unified INT8 via `sherpa-onnx` |
| Privacy | Local-first, no server required |

## 2. Problem

Typing is slow for long thoughts, prompts, emails, notes, and code-related explanations. Existing tools like Wispr Flow provide excellent UX but are closed-source and cloud-dependent. Built-in dictation is available but often lacks reliable formatting, correction handling, customization, and cross-app consistency.

Users need a local, hackable, privacy-respecting dictation layer that works across Windows and Linux.

## 3. Goals

- Provide fast push-to-talk dictation across desktop apps.
- Run STT locally without requiring internet.
- Support Ubuntu GNOME Wayland and Windows from the start.
- Use strong local models such as Parakeet via `sherpa-onnx`.
- Insert dictated text into the active app with minimal friction.
- Provide a foundation for future features like snippets, dictionary, command mode, and local LLM cleanup.
- Keep the app open-source and modular.

## 4. Non-Goals For MVP

- Mobile support.
- Cloud transcription.
- Full Wispr Flow feature parity.
- Team/admin dashboards.
- Perfect Wayland direct typing support.
- Training custom STT models.
- Real-time partial transcription overlay as a required MVP feature.
- Deep IDE context awareness in the first release.

## 5. Target Users

- Developers who want to dictate prompts, commit messages, issue descriptions, and notes.
- Linux users who want a local dictation tool that works beyond browser fields.
- Windows users who want offline speech-to-text.
- Privacy-conscious users.
- Accessibility users who need keyboard-light workflows.
- Power users who want configurable models, hotkeys, and text cleanup.

## 6. User Stories

- As a user, I can hold a hotkey, speak, release it, and have text pasted into the current app.
- As a user, I can use the app without internet access after models are downloaded.
- As a user, I can choose my microphone.
- As a user, I can see whether the app is idle, recording, transcribing, or pasting.
- As a user, I can recover the last transcript if insertion fails.
- As a user, I can change the global shortcut.
- As a user, I can copy the transcript instead of auto-pasting.
- As a user, I can configure whether cleanup is raw, basic, or AI-polished.
- As a Linux Wayland user, I can still use the app through clipboard paste fallback.
- As a Windows user, I can get more reliable active-window insertion.

## 7. MVP Scope

The MVP should include:

| Feature | MVP Requirement |
|---|---|
| Global shortcut | Push-to-talk hotkey |
| Audio capture | Microphone recording through OS audio stack |
| VAD | Detect speech boundaries using Silero VAD |
| STT | Local transcription through `sherpa-onnx` |
| Model | Parakeet INT8 ONNX model support |
| Text insertion | Clipboard + paste simulation |
| Overlay | Small status bubble/window |
| Settings | Hotkey, mic, model path, paste mode |
| Transcript recovery | Copy/paste last transcript |
| Packaging | Linux AppImage/deb and Windows installer |
| Privacy | No network calls except model download |

## 8. Platform Requirements

Linux target:

| Area | Requirement |
|---|---|
| Distro | Ubuntu 26.04 first |
| Desktop | GNOME Wayland first |
| Audio | PipeWire/WirePlumber |
| Hotkeys | Global shortcut support where possible |
| Text insertion | Clipboard paste fallback required |
| Portal | Use `xdg-desktop-portal` where appropriate |
| X11 | Add direct typing support later |

Windows target:

| Area | Requirement |
|---|---|
| OS | Windows 10/11 |
| Audio | WASAPI or cross-platform audio layer |
| Hotkeys | Native global hotkey |
| Text insertion | Clipboard paste + `SendInput` |
| GPU | Optional CUDA/DirectML later |
| Installer | `.msi` or `.exe` installer |

## 9. Current Ubuntu System Assumptions

The user's current system is:

| Area | Detected |
|---|---|
| OS | Ubuntu 26.04 LTS |
| Desktop | GNOME Shell 50.1 |
| Session | Wayland |
| Audio | PipeWire 1.6.2 |
| CPU | Intel i7-13700HX |
| GPU | NVIDIA RTX 3050 Laptop 6GB |
| CUDA | 13.2 |
| RAM | 14GB |
| Disk free | 137GB |

PRD implication: MVP must work well on **GNOME Wayland** with **clipboard-first insertion**.

## 10. Product UX

Primary flow:

1. User focuses a text field in any app.
2. User holds the configured hotkey.
3. Overlay changes to “Recording”.
4. User speaks naturally.
5. User releases hotkey.
6. App stops recording and transcribes locally.
7. App optionally cleans/formats text.
8. App pastes text into the active app.
9. If paste fails, app keeps text in clipboard and shows a notification.

Hands-free flow:

1. User taps shortcut once or double-taps configured shortcut.
2. App records until silence or user stops.
3. App transcribes and pastes.

Failure flow:

1. Text insertion fails.
2. App shows “Copied to clipboard”.
3. User manually presses paste.
4. User can trigger “Paste last transcript” shortcut.

## 11. Core Features

### 11.1 Push-To-Talk

- User can configure a global hotkey.
- Holding the hotkey records audio.
- Releasing the hotkey ends recording.
- `Esc` cancels current recording.
- Overlay must show active recording state.

### 11.2 Audio Capture

- App must detect available microphones.
- User can select a default microphone.
- App should show basic input level feedback.
- Linux should use PipeWire-compatible capture.
- Windows should use WASAPI-compatible capture.

### 11.3 Voice Activity Detection

- Use Silero VAD through ONNX.
- Trim leading/trailing silence.
- Prevent sending empty audio to STT.
- Optional auto-stop after silence in hands-free mode.

### 11.4 Local Transcription

- Use `sherpa-onnx`.
- Support Parakeet ONNX models.
- Default model should be one of:

| Model | Use |
|---|---|
| `parakeet-tdt-0.6b-v3-int8` | Default multilingual |
| `parakeet-tdt-0.6b-v2-int8` | English-focused |
| `parakeet-unified-en-0.6b-int8` | English streaming/non-streaming candidate |

### 11.5 Text Cleanup

MVP cleanup levels:

| Mode | Behavior |
|---|---|
| Raw | Paste exact STT output |
| Basic | Trim whitespace, fix spacing, remove obvious duplicate spaces |
| Smart | Basic cleanup plus simple correction rules |
| LLM Polish | Future optional local LLM pass |

MVP should start with `Raw` and `Basic`.

### 11.6 Text Insertion

Insertion methods:

| Method | Platform | Priority |
|---|---|---|
| Clipboard + paste hotkey | Linux/Windows | Required |
| Native simulated typing | Windows | Required eventually |
| X11 direct typing | Linux X11 | Post-MVP |
| Wayland portal-based path | Linux Wayland | Research/post-MVP |

On Wayland, clipboard paste is the baseline.

### 11.7 Overlay

Overlay states:

| State | UI |
|---|---|
| Idle | Small mic icon |
| Recording | Animated indicator |
| Processing | Spinner |
| Pasted | Success flash |
| Error | Short message |
| Copied | Clipboard confirmation |

Overlay must not steal focus.

### 11.8 Settings

Settings MVP:

| Setting | Requirement |
|---|---|
| Hotkey | Configure push-to-talk |
| Microphone | Select input device |
| Model | Select/download model |
| Paste mode | Auto-paste or copy-only |
| Cleanup | Raw/basic |
| Start on login | Optional |
| Theme | System/default |

### 11.9 Last Transcript

- Store last transcript locally.
- Provide “copy last transcript”.
- Provide “paste last transcript”.
- Do not sync transcript history.

## 12. Future Features

Post-MVP feature backlog:

| Feature | Notes |
|---|---|
| Snippets | Spoken trigger expands to saved text |
| Dictionary | Custom words, names, jargon |
| Backtrack | “Actually”, “scratch that”, restatement cleanup |
| Command Mode | Highlight text, speak instruction, replace selected text |
| Local LLM polish | Grammar, tone, format cleanup |
| App-aware styles | Casual in chat, formal in email |
| IDE mode | Code symbols, filenames, prompt dictation |
| Multi-language model manager | Download/switch models |
| Wake word | Optional hands-free activation |
| Dictation history | Local-only, user-controlled |
| Export/import settings | JSON config |
| Plugin system | Community transforms and integrations |

## 13. Technical Architecture

Recommended stack:

| Layer | Choice |
|---|---|
| App shell | Tauri |
| Core language | Rust |
| UI | Web frontend inside Tauri |
| STT runtime | `sherpa-onnx` |
| Audio | `cpal` or native backend |
| VAD | Silero ONNX |
| Storage | SQLite |
| Config | TOML/JSON |
| Packaging | Tauri bundler |
| Linux audio | PipeWire-compatible |
| Windows audio | WASAPI-compatible |
| Linux insertion | Clipboard + portal/X11 adapters |
| Windows insertion | Clipboard + `SendInput` |

Proposed module layout:

```text
flowoss/
  apps/
    desktop/
  crates/
    core/
    audio/
    stt/
    vad/
    text_cleanup/
    insertion/
    hotkeys/
    overlay/
    model_manager/
    settings/
  platform/
    linux/
    windows/
  models/
  docs/
```

## 14. Model Management

Requirements:

| Requirement | Description |
|---|---|
| Bundled model | No large model bundled in source repo |
| First-run download | App offers to download selected model |
| Manual model path | User can point to existing model folder |
| Checksums | Verify model integrity |
| Disk warning | Show model size before download |
| Offline use | App works offline after setup |

Model sizes are expected around `600MB-1.3GB` depending on model.

## 15. Privacy Requirements

- No audio leaves the machine by default.
- No transcript leaves the machine by default.
- No telemetry in MVP.
- Model downloads require explicit user action.
- Transcript history is local and optional.
- User can clear last transcript/history.

## 16. Performance Requirements

Target performance on current Ubuntu machine:

| Metric | Target |
|---|---|
| Startup | Under 3 seconds after warm install |
| Hotkey response | Under 100ms |
| Recording start | Under 200ms |
| Short dictation processing | Under 2 seconds preferred |
| Empty audio rejection | Under 500ms |
| Model load | Accept slower first load, then keep warm |

For Parakeet INT8 CPU mode, short utterances should be usable. GPU acceleration is optional for MVP.

## 17. Accessibility Requirements

- Keyboard-first operation.
- Large status indicators.
- Clear notification if mic permission or audio capture fails.
- Copy fallback always available.
- No requirement to use mouse during normal dictation.

## 18. Open-Source Requirements

Suggested license:

| Component | License Direction |
|---|---|
| App code | MIT or Apache-2.0 |
| Models | Respect upstream model licenses |
| Parakeet v2/v3 | CC-BY-4.0 attribution required |
| Sherpa ONNX | Apache-2.0 |
| Silero VAD | Check upstream license before bundling |

Need include attribution for NVIDIA Parakeet models if distributed or downloaded through app.

## 19. Acceptance Criteria

MVP is accepted when:

- App installs on Ubuntu GNOME Wayland.
- App installs on Windows 10/11.
- User can configure a push-to-talk hotkey.
- User can select a microphone.
- User can record a short utterance.
- User can transcribe locally with Parakeet ONNX.
- User can paste result into a text field using clipboard paste.
- User can recover last transcript.
- App works offline after model download.
- App shows clear recording/processing/error states.
- No cloud call is made during dictation.

## 20. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Wayland blocks direct typing | High | Clipboard fallback first |
| Global hotkeys on Wayland inconsistent | Medium | Use Tauri/global shortcut where possible, document limitations |
| Model too large for some users | Medium | Offer smaller fallback models later |
| CPU latency too high | Medium | Keep model warm, offer CUDA later |
| Paste fails in secure fields | Low | Copy fallback and notification |
| Packaging ONNX Runtime complexity | Medium | Use `sherpa-onnx` prebuilt binaries or controlled build |
| NVIDIA model license confusion | Medium | Clear attribution and model download flow |

## 21. Milestones

| Milestone | Scope |
|---|---|
| M0 Research Spike | Prove `sherpa-onnx` Parakeet transcription locally |
| M1 CLI Prototype | Record mic, VAD, transcribe, print text |
| M2 Paste Prototype | Hotkey, record, transcribe, paste into active app |
| M3 Desktop Shell | Tauri UI, overlay, settings |
| M4 Linux MVP | Ubuntu GNOME Wayland clipboard workflow |
| M5 Windows MVP | Windows hotkey and paste workflow |
| M6 Model Manager | Download/select/manage models |
| M7 Public Alpha | Installers, README, known limitations |
| M8 Smart Features | Dictionary, snippets, cleanup rules |

## 22. MVP Build Plan

Recommended first implementation order:

1. Build CLI proof of concept with `sherpa-onnx` and Parakeet.
2. Add microphone capture.
3. Add VAD.
4. Add push-to-talk.
5. Add clipboard paste.
6. Add minimal overlay.
7. Add Tauri settings UI.
8. Package for Ubuntu.
9. Package for Windows.
10. Add model manager.

## 23. First Technical Decision

Use `sherpa-onnx` as the STT runtime for MVP.

Reason:

- Already supports Parakeet ONNX conversions.
- Cross-platform.
- Avoids Python/PyTorch/NeMo deployment.
- Has VAD and microphone examples.
- Good fit for Rust/native integration through C/C++ bindings.

## 24. Initial MVP Name

Temporary internal name: **FlowOSS**

Possible final names:

| Name | Notes |
|---|---|
| FlowOSS | Clear positioning |
| LocalFlow | Emphasizes local-first |
| VoxFlow | Voice-flow concept |
| SpeakType | Literal, simple |
| OpenDictate | Descriptive |
| Whisperless | Avoids Whisper dependency but maybe confusing |

## 25. Final Product Principle

The product should feel like:

> Hold a key, speak naturally, release, text appears.

Everything else should support that loop without making the user think about models, files, audio chunks, or OS limitations.
