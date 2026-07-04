<#
.SYNOPSIS
    Download the default FlowOSS models (explicit user action per PRD s14/s15).

.DESCRIPTION
    Fetches, into %APPDATA%\flowoss\models by default:
      - NVIDIA Parakeet TDT 0.6b v2 int8 (English), ~700MB, CC-BY-4.0
      - Silero VAD, ~2MB (https://github.com/snakers4/silero-vad)
      - Streaming Zipformer EN 20M, ~90MB (live overlay preview)
    All are served from sherpa-onnx release assets.

    Override the target with the FLOWOSS_MODELS_DIR environment variable.
#>
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # keeps Invoke-WebRequest fast on big files

$ModelsDir = if ($env:FLOWOSS_MODELS_DIR) { $env:FLOWOSS_MODELS_DIR } else { Join-Path $env:APPDATA "flowoss\models" }
$BaseUrl = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
$SttModel = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8"
$StreamModel = "sherpa-onnx-streaming-zipformer-en-20M-2023-02-17"

New-Item -ItemType Directory -Force -Path $ModelsDir | Out-Null
Set-Location $ModelsDir

function Get-File($url, $out) {
    Write-Host "Downloading $out ..."
    # curl.exe (bundled on Windows 10+) streams large files faster than IWR.
    if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
        curl.exe -SL -o $out $url
    } else {
        Invoke-WebRequest -Uri $url -OutFile $out
    }
}

function Expand-Tar($archive) {
    Write-Host "Extracting $archive ..."
    # bsdtar (bundled on Windows 10+) auto-detects the bzip2 compression.
    tar -xf $archive
    Remove-Item $archive -Force
}

if (-not (Test-Path "silero_vad.onnx")) {
    Get-File "$BaseUrl/silero_vad.onnx" "silero_vad.onnx"
} else {
    Write-Host "Silero VAD already present."
}

if (-not (Test-Path $StreamModel)) {
    Get-File "$BaseUrl/$StreamModel.tar.bz2" "$StreamModel.tar.bz2"
    Expand-Tar "$StreamModel.tar.bz2"
} else {
    Write-Host "$StreamModel already present."
}

if (-not (Test-Path $SttModel)) {
    Get-File "$BaseUrl/$SttModel.tar.bz2" "$SttModel.tar.bz2"
    Expand-Tar "$SttModel.tar.bz2"
} else {
    Write-Host "$SttModel already present."
}

Write-Host "Models ready in $ModelsDir"
