//! Procedurally synthesized, license-free desktop feedback sounds.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Short, related mechanical cues used as supplemental feedback by the
/// desktop app. They are synthesized instead of loaded from assets so the
/// sound set stays license-free and works in packaged/offline builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackCue {
    RecordStart,
    RecordStop,
    AssistStart,
    Success,
    AssistAnswer,
    NoSpeech,
    Cancel,
    Error,
}

struct Playback {
    samples: Vec<f32>,
    cursor: usize,
}

/// A warm output stream that can play a cue without adding latency to the
/// hotkey path. New cues supersede old ones, preventing overlapping sounds.
pub struct FeedbackPlayer {
    _stream: cpal::Stream,
    playback: Arc<Mutex<Playback>>,
    sample_rate: u32,
}

impl FeedbackPlayer {
    pub fn new() -> Result<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device found"))?;
        let supported = device
            .default_output_config()
            .context("failed to query default output config")?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let playback = Arc::new(Mutex::new(Playback {
            samples: Vec::new(),
            cursor: 0,
        }));
        let err_fn = |e| eprintln!("feedback audio stream error: {e}");

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let state = playback.clone();
                device.build_output_stream(
                    supported.clone().into(),
                    move |data: &mut [f32], _: &_| fill_output(data, channels, &state, |v| v),
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let state = playback.clone();
                device.build_output_stream(
                    supported.clone().into(),
                    move |data: &mut [i16], _: &_| {
                        fill_output(data, channels, &state, |v| {
                            (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                        })
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::U16 => {
                let state = playback.clone();
                device.build_output_stream(
                    supported.clone().into(),
                    move |data: &mut [u16], _: &_| {
                        fill_output(data, channels, &state, |v| {
                            ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16
                        })
                    },
                    err_fn,
                    None,
                )?
            }
            other => bail!("unsupported output sample format: {other}"),
        };
        stream.play().context("failed to start feedback output")?;
        Ok(Self {
            _stream: stream,
            playback,
            sample_rate,
        })
    }

    pub fn play(&self, cue: FeedbackCue, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        let mut samples = synthesize(cue, self.sample_rate);
        for sample in &mut samples {
            *sample *= volume;
        }
        if let Ok(mut playback) = self.playback.lock() {
            playback.samples = samples;
            playback.cursor = 0;
        }
    }
}

fn fill_output<T: Copy>(
    output: &mut [T],
    channels: usize,
    state: &Arc<Mutex<Playback>>,
    convert: impl Fn(f32) -> T,
) {
    let Ok(mut playback) = state.lock() else {
        return;
    };
    for frame in output.chunks_mut(channels.max(1)) {
        let sample = playback
            .samples
            .get(playback.cursor)
            .copied()
            .unwrap_or(0.0);
        if playback.cursor < playback.samples.len() {
            playback.cursor += 1;
        }
        for channel in frame {
            *channel = convert(sample);
        }
    }
}

fn synthesize(cue: FeedbackCue, rate: u32) -> Vec<f32> {
    // The family is based on a typewriter mechanism: a dry key strike, a
    // damped body resonance, and (for completion) a carriage bell. Direction
    // is encoded consistently: rising/bright means start or success;
    // descending/damped means stop, cancellation, or failure.
    let seconds = match cue {
        FeedbackCue::RecordStart => 0.105,
        FeedbackCue::RecordStop => 0.120,
        FeedbackCue::AssistStart => 0.155,
        FeedbackCue::Success => 0.300,
        FeedbackCue::AssistAnswer => 0.340,
        FeedbackCue::NoSpeech => 0.230,
        FeedbackCue::Cancel => 0.180,
        FeedbackCue::Error => 0.260,
    };
    let mut out = vec![0.0; (seconds * rate as f32) as usize];
    match cue {
        FeedbackCue::RecordStart => {
            key_strike(&mut out, rate, 0.000, 0.25, 1);
            tone(&mut out, rate, 0.012, 0.080, 760.0, 0.075, 35.0);
        }
        FeedbackCue::RecordStop => {
            key_strike(&mut out, rate, 0.000, 0.18, 2);
            tone(&mut out, rate, 0.008, 0.090, 620.0, 0.060, 28.0);
        }
        FeedbackCue::AssistStart => {
            key_strike(&mut out, rate, 0.000, 0.20, 3);
            key_strike(&mut out, rate, 0.052, 0.14, 4);
            tone(&mut out, rate, 0.020, 0.115, 820.0, 0.055, 19.0);
            tone(&mut out, rate, 0.064, 0.080, 1080.0, 0.045, 24.0);
        }
        FeedbackCue::Success => {
            key_strike(&mut out, rate, 0.000, 0.14, 5);
            tone(&mut out, rate, 0.018, 0.265, 1046.5, 0.13, 10.0);
            tone(&mut out, rate, 0.018, 0.230, 1569.8, 0.055, 12.0);
        }
        FeedbackCue::AssistAnswer => {
            key_strike(&mut out, rate, 0.000, 0.12, 6);
            key_strike(&mut out, rate, 0.070, 0.10, 7);
            tone(&mut out, rate, 0.024, 0.290, 880.0, 0.085, 9.0);
            tone(&mut out, rate, 0.092, 0.220, 1320.0, 0.075, 11.0);
        }
        FeedbackCue::NoSpeech => {
            key_strike(&mut out, rate, 0.000, 0.10, 8);
            key_strike(&mut out, rate, 0.092, 0.075, 9);
            tone(&mut out, rate, 0.008, 0.090, 650.0, 0.040, 23.0);
            tone(&mut out, rate, 0.100, 0.105, 520.0, 0.036, 21.0);
        }
        FeedbackCue::Cancel => {
            key_strike(&mut out, rate, 0.000, 0.13, 10);
            key_strike(&mut out, rate, 0.047, 0.085, 11);
            tone(&mut out, rate, 0.006, 0.145, 560.0, 0.045, 20.0);
        }
        FeedbackCue::Error => {
            key_strike(&mut out, rate, 0.000, 0.26, 12);
            key_strike(&mut out, rate, 0.035, 0.15, 13);
            tone(&mut out, rate, 0.010, 0.210, 620.0, 0.075, 13.0);
            tone(&mut out, rate, 0.010, 0.210, 523.3, 0.055, 13.0);
        }
    }
    // Leave headroom even when resonances coincide.
    for sample in &mut out {
        *sample = soft_clip(*sample * 1.4) * 0.72;
    }
    out
}

fn key_strike(out: &mut [f32], rate: u32, start: f32, amplitude: f32, mut seed: u32) {
    let begin = (start * rate as f32) as usize;
    let length = (0.025 * rate as f32) as usize;
    let mut previous = 0.0;
    for i in 0..length.min(out.len().saturating_sub(begin)) {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 8) as f32 / 8_388_607.5) - 1.0;
        // Difference successive values to favor the crisp mechanical band.
        let bright = noise - previous * 0.72;
        previous = noise;
        let t = i as f32 / rate as f32;
        let envelope = (-t * 155.0).exp();
        let body = (std::f32::consts::TAU * 690.0 * t).sin() * (-t * 75.0).exp();
        out[begin + i] += amplitude * envelope * (bright * 0.62 + body * 0.38);
    }
}

fn tone(
    out: &mut [f32],
    rate: u32,
    start: f32,
    duration: f32,
    frequency: f32,
    amplitude: f32,
    decay: f32,
) {
    let begin = (start * rate as f32) as usize;
    let length = (duration * rate as f32) as usize;
    for i in 0..length.min(out.len().saturating_sub(begin)) {
        let t = i as f32 / rate as f32;
        let attack = (t * 240.0).min(1.0);
        let envelope = attack * (-t * decay).exp();
        let fundamental = (std::f32::consts::TAU * frequency * t).sin();
        let overtone = (std::f32::consts::TAU * frequency * 2.01 * t).sin();
        out[begin + i] += amplitude * envelope * (fundamental + overtone * 0.22);
    }
}

fn soft_clip(value: f32) -> f32 {
    value / (1.0 + value.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_audible_and_bounded() {
        let cues = [
            FeedbackCue::RecordStart,
            FeedbackCue::RecordStop,
            FeedbackCue::AssistStart,
            FeedbackCue::Success,
            FeedbackCue::AssistAnswer,
            FeedbackCue::NoSpeech,
            FeedbackCue::Cancel,
            FeedbackCue::Error,
        ];
        for cue in cues {
            let samples = synthesize(cue, 48_000);
            assert!(!samples.is_empty(), "{cue:?}");
            assert!(
                samples.len() < 48_000,
                "{cue:?} must remain under one second"
            );
            assert!(
                samples.iter().all(|sample| sample.abs() <= 1.0),
                "{cue:?} clipped"
            );
            assert!(
                samples.iter().any(|sample| sample.abs() > 0.001),
                "{cue:?}"
            );
        }
    }
}
