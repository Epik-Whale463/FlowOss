//! Local speech-to-text via sherpa-onnx (PRD 11.4).
//!
//! Wraps the offline transducer recognizer with `model_type =
//! "nemo_transducer"`, which is how sherpa-onnx runs NVIDIA Parakeet TDT
//! ONNX exports.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use flowoss_core::SAMPLE_RATE;
use sherpa_rs::transducer::{TransducerConfig, TransducerRecognizer};

mod streaming;
pub use streaming::StreamingTranscriber;

pub struct Transcriber {
    recognizer: TransducerRecognizer,
}

impl Transcriber {
    /// Load a Parakeet transducer model from a directory containing
    /// `encoder*.onnx`, `decoder*.onnx`, `joiner*.onnx` and `tokens.txt`
    /// (the layout of sherpa-onnx release tarballs). Prefers int8 variants.
    pub fn from_model_dir(dir: &Path, num_threads: i32) -> Result<Self> {
        let encoder = find_model_file(dir, "encoder")?;
        let decoder = find_model_file(dir, "decoder")?;
        let joiner = find_model_file(dir, "joiner")?;
        let tokens = dir.join("tokens.txt");
        if !tokens.exists() {
            bail!("tokens.txt not found in {}", dir.display());
        }

        let config = TransducerConfig {
            encoder: encoder.to_string_lossy().into_owned(),
            decoder: decoder.to_string_lossy().into_owned(),
            joiner: joiner.to_string_lossy().into_owned(),
            tokens: tokens.to_string_lossy().into_owned(),
            model_type: "nemo_transducer".into(),
            decoding_method: "greedy_search".into(),
            sample_rate: SAMPLE_RATE as i32,
            feature_dim: 80,
            num_threads,
            ..TransducerConfig::default()
        };
        let recognizer = TransducerRecognizer::new(config).map_err(|e| {
            anyhow::anyhow!("failed to load STT model from {}: {e}", dir.display())
        })?;
        Ok(Self { recognizer })
    }

    /// Transcribe 16 kHz mono samples.
    pub fn transcribe(&mut self, samples: &[f32]) -> String {
        self.recognizer
            .transcribe(SAMPLE_RATE, samples)
            .trim()
            .to_string()
    }
}

/// Find `<stem>.int8.onnx` (preferred) or `<stem>.onnx` in `dir`.
pub(crate) fn find_model_file(dir: &Path, stem: &str) -> Result<PathBuf> {
    for name in [format!("{stem}.int8.onnx"), format!("{stem}.onnx")] {
        let path = dir.join(&name);
        if path.exists() {
            return Ok(path);
        }
    }
    // Fall back to any file starting with the stem and ending in .onnx
    // (some exports use names like encoder-epoch-99.onnx); prefer int8.
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("model directory not found: {}", dir.display()))?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            name.starts_with(stem) && name.ends_with(".onnx")
        })
        .collect();
    candidates.sort_by_key(|p| !p.to_string_lossy().contains(".int8."));
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no {stem}*.onnx found in {}", dir.display()))
}
