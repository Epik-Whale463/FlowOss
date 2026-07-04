//! First-run model download.
//!
//! The speech models (~800MB) are intentionally not shipped in the app. On
//! first launch, if they're missing, we fetch them from the sherpa-onnx
//! release assets into the user's models directory and report progress so the
//! overlay can show a download pill. Archives are extracted with the `tar`
//! that ships with Windows 10 1803+ (and every Linux).

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::settings::Settings;

const BASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

/// True once the required models (Parakeet STT + Silero VAD) are on disk.
pub fn models_present(settings: &Settings) -> bool {
    settings.model_dir.join("tokens.txt").exists() && settings.vad_model.exists()
}

/// Download any missing models. `progress(label, percent)` is called as bytes
/// arrive so the caller can surface a progress indicator.
pub fn ensure_models(settings: &Settings, mut progress: impl FnMut(&str, u32)) -> Result<()> {
    let models_root = flowoss_core::models_dir();
    std::fs::create_dir_all(&models_root)?;

    if !settings.vad_model.exists() {
        download_file(
            &format!("{BASE_URL}/silero_vad.onnx"),
            &settings.vad_model,
            "Voice-activity model",
            &mut progress,
        )?;
    }

    if !settings.model_dir.join("tokens.txt").exists() {
        download_and_extract(&settings.model_dir, &models_root, "Speech model", &mut progress)?;
    }

    Ok(())
}

fn download_and_extract(
    model_dir: &Path,
    models_root: &Path,
    label: &str,
    progress: &mut dyn FnMut(&str, u32),
) -> Result<()> {
    let name = model_dir
        .file_name()
        .and_then(|n| n.to_str())
        .context("model directory has no name")?;
    let archive = models_root.join(format!("{name}.tar.bz2"));
    download_file(
        &format!("{BASE_URL}/{name}.tar.bz2"),
        &archive,
        label,
        progress,
    )?;
    progress(&format!("Extracting {label}…"), 100);
    extract_tar_bz2(&archive, models_root)?;
    let _ = std::fs::remove_file(&archive);
    Ok(())
}

fn download_file(
    url: &str,
    dest: &Path,
    label: &str,
    progress: &mut dyn FnMut(&str, u32),
) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let mut reader = response.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut last_pct = u32::MAX;
    progress(label, 0);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if total > 0 {
            let pct = ((done * 100) / total) as u32;
            if pct != last_pct {
                progress(label, pct);
                last_pct = pct;
            }
        }
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, dest).with_context(|| format!("finalizing {}", dest.display()))?;
    Ok(())
}

fn extract_tar_bz2(archive: &Path, dest_dir: &Path) -> Result<()> {
    // bsdtar auto-detects bzip2; it's bundled on Windows 10 1803+ and Linux.
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest_dir)
        .status()
        .context("running `tar` (needs Windows 10 1803+ or bsdtar on PATH)")?;
    if !status.success() {
        bail!("tar failed to extract {}", archive.display());
    }
    Ok(())
}
