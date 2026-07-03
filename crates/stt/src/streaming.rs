//! Streaming (online) transcription for live word-by-word preview.
//!
//! sherpa-rs doesn't wrap the online recognizer, so this goes straight to
//! the sherpa-onnx C API. A small streaming Zipformer model provides
//! low-latency partial results while recording; the accurate final
//! transcript still comes from the offline Parakeet pass.

use std::ffi::{CStr, CString};
use std::mem;
use std::path::Path;

use anyhow::{bail, Result};
use flowoss_core::SAMPLE_RATE;

use crate::find_model_file;

pub struct StreamingTranscriber {
    recognizer: *const sherpa_rs_sys::SherpaOnnxOnlineRecognizer,
    stream: *const sherpa_rs_sys::SherpaOnnxOnlineStream,
    // Keep the config strings alive for the recognizer's lifetime; the C
    // API copies them at create time, but this is cheap insurance.
    _strings: Vec<CString>,
}

// The recognizer is only ever used from the engine thread; the raw
// pointers are what stop Send from being derived.
unsafe impl Send for StreamingTranscriber {}

impl StreamingTranscriber {
    /// Load a streaming transducer (Zipformer) model directory containing
    /// `encoder*.onnx`, `decoder*.onnx`, `joiner*.onnx`, `tokens.txt`.
    pub fn from_model_dir(dir: &Path, num_threads: i32) -> Result<Self> {
        let encoder = CString::new(find_model_file(dir, "encoder")?.to_string_lossy().as_bytes())?;
        let decoder = CString::new(find_model_file(dir, "decoder")?.to_string_lossy().as_bytes())?;
        let joiner = CString::new(find_model_file(dir, "joiner")?.to_string_lossy().as_bytes())?;
        let tokens_path = dir.join("tokens.txt");
        if !tokens_path.exists() {
            bail!("tokens.txt not found in {}", dir.display());
        }
        let tokens = CString::new(tokens_path.to_string_lossy().as_bytes())?;
        let decoding = CString::new("greedy_search")?;
        let provider = CString::new("cpu")?;

        let (recognizer, stream) = unsafe {
            let mut config: sherpa_rs_sys::SherpaOnnxOnlineRecognizerConfig = mem::zeroed();
            config.feat_config.sample_rate = SAMPLE_RATE as i32;
            config.feat_config.feature_dim = 80;
            config.model_config.transducer.encoder = encoder.as_ptr();
            config.model_config.transducer.decoder = decoder.as_ptr();
            config.model_config.transducer.joiner = joiner.as_ptr();
            config.model_config.tokens = tokens.as_ptr();
            config.model_config.num_threads = num_threads;
            config.model_config.provider = provider.as_ptr();
            config.decoding_method = decoding.as_ptr();

            let recognizer = sherpa_rs_sys::SherpaOnnxCreateOnlineRecognizer(&config);
            if recognizer.is_null() {
                bail!("failed to create online recognizer from {}", dir.display());
            }
            let stream = sherpa_rs_sys::SherpaOnnxCreateOnlineStream(recognizer);
            if stream.is_null() {
                sherpa_rs_sys::SherpaOnnxDestroyOnlineRecognizer(recognizer);
                bail!("failed to create online stream");
            }
            (recognizer, stream)
        };

        Ok(Self {
            recognizer,
            stream,
            _strings: vec![encoder, decoder, joiner, tokens, decoding, provider],
        })
    }

    /// Feed 16 kHz mono samples and return the current partial transcript.
    pub fn feed(&mut self, samples: &[f32]) -> String {
        unsafe {
            if !samples.is_empty() {
                sherpa_rs_sys::SherpaOnnxOnlineStreamAcceptWaveform(
                    self.stream,
                    SAMPLE_RATE as i32,
                    samples.as_ptr(),
                    samples.len() as i32,
                );
            }
            while sherpa_rs_sys::SherpaOnnxIsOnlineStreamReady(self.recognizer, self.stream) == 1 {
                sherpa_rs_sys::SherpaOnnxDecodeOnlineStream(self.recognizer, self.stream);
            }
            let result = sherpa_rs_sys::SherpaOnnxGetOnlineStreamResult(self.recognizer, self.stream);
            if result.is_null() {
                return String::new();
            }
            let text = if (*result).text.is_null() {
                String::new()
            } else {
                CStr::from_ptr((*result).text).to_string_lossy().into_owned()
            };
            sherpa_rs_sys::SherpaOnnxDestroyOnlineRecognizerResult(result);
            text
        }
    }

    /// Clear state between utterances.
    pub fn reset(&mut self) {
        unsafe {
            sherpa_rs_sys::SherpaOnnxOnlineStreamReset(self.recognizer, self.stream);
        }
    }
}

impl Drop for StreamingTranscriber {
    fn drop(&mut self) {
        unsafe {
            sherpa_rs_sys::SherpaOnnxDestroyOnlineStream(self.stream);
            sherpa_rs_sys::SherpaOnnxDestroyOnlineRecognizer(self.recognizer);
        }
    }
}
