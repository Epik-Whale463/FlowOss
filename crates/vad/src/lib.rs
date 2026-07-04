//! Voice activity detection via Silero VAD (PRD 11.3).
//!
//! Trims leading/trailing silence and drops recordings that contain no
//! speech, so empty audio never reaches the STT engine.

use std::path::Path;

use anyhow::{anyhow, Result};
use flowoss_core::SAMPLE_RATE;
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};

const WINDOW_SIZE: usize = 512; // samples per VAD window at 16 kHz
const SEGMENT_GAP_SECS: f32 = 0.2; // silence inserted between speech segments

pub struct SpeechDetector {
    vad: SileroVad,
}

impl SpeechDetector {
    pub fn new(model_path: &Path) -> Result<Self> {
        let config = SileroVadConfig {
            model: model_path.to_string_lossy().into_owned(),
            threshold: 0.5,
            min_silence_duration: 0.5,
            min_speech_duration: 0.25,
            max_speech_duration: 30.0,
            sample_rate: SAMPLE_RATE,
            window_size: WINDOW_SIZE as i32,
            provider: None,
            num_threads: Some(1),
            debug: false,
        };
        let vad = SileroVad::new(config, 60.0).map_err(|e| {
            anyhow!("failed to load VAD model {}: {e}", model_path.display())
        })?;
        Ok(Self { vad })
    }

    /// Extract speech from 16 kHz mono samples. Returns the concatenated
    /// speech segments (with short gaps between them), or `None` if no
    /// speech was detected.
    pub fn extract_speech(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        self.vad.clear();
        for chunk in samples.chunks(WINDOW_SIZE) {
            let mut window = chunk.to_vec();
            window.resize(WINDOW_SIZE, 0.0); // zero-pad the final partial window
            self.vad.accept_waveform(window);
        }
        self.vad.flush();

        let gap = vec![0.0f32; (SAMPLE_RATE as f32 * SEGMENT_GAP_SECS) as usize];
        let mut speech: Vec<f32> = Vec::new();
        while !self.vad.is_empty() {
            let segment = self.vad.front();
            if !speech.is_empty() {
                speech.extend_from_slice(&gap);
            }
            speech.extend_from_slice(&segment.samples);
            self.vad.pop();
        }

        if speech.is_empty() {
            None
        } else {
            Some(speech)
        }
    }
}
