//! Shared types and paths for FlowOSS.

use std::path::PathBuf;

/// Sample rate all internal audio is normalized to (Parakeet + Silero VAD).
pub const SAMPLE_RATE: u32 = 16_000;

/// Default Parakeet model directory name, as extracted from the sherpa-onnx
/// release tarball.
pub const DEFAULT_STT_MODEL: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8";

/// Root data directory: `~/.local/share/flowoss` on Linux,
/// `%APPDATA%\flowoss` on Windows.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flowoss")
}

/// Directory where downloaded models live.
pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

/// Default directory of the STT model.
pub fn default_stt_model_dir() -> PathBuf {
    models_dir().join(DEFAULT_STT_MODEL)
}

/// Default path of the Silero VAD model.
pub fn default_vad_model_path() -> PathBuf {
    models_dir().join("silero_vad.onnx")
}
