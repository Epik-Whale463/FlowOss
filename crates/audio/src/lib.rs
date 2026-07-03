//! Microphone capture and WAV loading (PRD 11.2).
//!
//! All audio leaving this crate is 16 kHz mono f32, the format expected by
//! both Silero VAD and Parakeet.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use flowoss_core::SAMPLE_RATE;

/// Names of all available input devices, default first.
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .input_devices()
        .context("failed to enumerate input devices")?
        .filter_map(|d| d.name().ok())
        .collect();
    if let Some(def) = default_name {
        names.retain(|n| *n != def);
        names.insert(0, def);
    }
    Ok(names)
}

fn find_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device found")),
        Some(wanted) => host
            .input_devices()?
            .find(|d| d.name().map(|n| n == wanted).unwrap_or(false))
            .ok_or_else(|| anyhow!("input device not found: {wanted}")),
    }
}

/// An in-progress microphone recording. Keep it on the thread that created it
/// (cpal streams are not `Send`); call [`Recording::stop`] to get the samples.
pub struct Recording {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    source_rate: u32,
}

impl Recording {
    /// Start capturing from the given device (or the default one).
    pub fn start(device_name: Option<&str>) -> Result<Self> {
        let device = find_device(device_name)?;
        let config = device
            .default_input_config()
            .context("failed to query default input config")?;
        let channels = config.channels() as usize;
        let source_rate = config.sample_rate().0;
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));

        let err_fn = |e| eprintln!("audio stream error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let buf = buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| push_mono(&buf, data, channels),
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let buf = buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| {
                        let floats: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        push_mono(&buf, &floats, channels);
                    },
                    err_fn,
                    None,
                )?
            }
            other => bail!("unsupported input sample format: {other}"),
        };
        stream.play().context("failed to start input stream")?;

        Ok(Self {
            stream,
            buffer,
            source_rate,
        })
    }

    /// Current input level (RMS of roughly the last 100 ms), 0.0..=1.0.
    pub fn level(&self) -> f32 {
        let buf = self.buffer.lock().unwrap();
        let window = (self.source_rate as usize) / 10;
        let tail = &buf[buf.len().saturating_sub(window)..];
        if tail.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = tail.iter().map(|s| s * s).sum();
        (sum_sq / tail.len() as f32).sqrt().min(1.0)
    }

    /// Seconds of audio captured so far.
    pub fn duration_secs(&self) -> f32 {
        self.buffer.lock().unwrap().len() as f32 / self.source_rate as f32
    }

    /// Stop the stream and return 16 kHz mono samples.
    pub fn stop(self) -> Vec<f32> {
        drop(self.stream);
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap());
        resample_linear(&samples, self.source_rate, SAMPLE_RATE)
    }
}

/// Downmix interleaved frames to mono and append to the shared buffer.
fn push_mono(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
    let mut buf = buffer.lock().unwrap();
    if channels <= 1 {
        buf.extend_from_slice(data);
    } else {
        buf.extend(
            data.chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }
}

/// Linear-interpolation resampler; good enough for speech input.
pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let a = input[idx];
        let b = *input.get(idx + 1).unwrap_or(&a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// Load a WAV file as 16 kHz mono f32.
pub fn load_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()?
        }
    };

    let mono: Vec<f32> = if channels <= 1 {
        interleaved
    } else {
        interleaved
            .chunks_exact(channels)
            .map(|f| f.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    Ok(resample_linear(&mono, spec.sample_rate, SAMPLE_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_halves_length() {
        let input = vec![0.0f32; 32000];
        assert_eq!(resample_linear(&input, 32000, 16000).len(), 16000);
    }

    #[test]
    fn resample_identity() {
        let input = vec![0.5f32; 100];
        assert_eq!(resample_linear(&input, 16000, 16000), input);
    }
}
