<#
.SYNOPSIS
    Build and package FlowOSS as a portable Windows ZIP.

.DESCRIPTION
    Produces dist\FlowOSS-windows-x64.zip containing the desktop app, the CLI,
    the required native DLLs, and a README. The models are NOT bundled; the app
    downloads them on first run. Requires the Rust MSVC toolchain and LLVM
    (libclang) for the sherpa-rs build.

.PARAMETER SkipBuild
    Package the existing target\release binaries without rebuilding.
#>
param([switch]$SkipBuild)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Release = Join-Path $RepoRoot "target\release"
$DistRoot = Join-Path $RepoRoot "dist"
$StageDir = Join-Path $DistRoot "FlowOSS"
$ZipPath = Join-Path $DistRoot "FlowOSS-windows-x64.zip"

if (-not $SkipBuild) {
    if (-not $env:LIBCLANG_PATH -and (Test-Path "C:\Program Files\LLVM\bin\libclang.dll")) {
        $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
    }
    Write-Host "Building release binaries..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    try { cargo build --release } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

# Fresh staging directory.
if (Test-Path $StageDir) { Remove-Item $StageDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

$payload = @(
    "flowoss-desktop.exe",   # the app
    "flowoss.exe",           # optional CLI (daemon, transcribe, devices)
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
    "cargs.dll"
)
foreach ($f in $payload) {
    $src = Join-Path $Release $f
    if (-not (Test-Path $src)) { throw "missing build artifact: $src (did the release build run?)" }
    Copy-Item $src (Join-Path $StageDir $f) -Force
}

Copy-Item (Join-Path $PSScriptRoot "..\packaging\windows\README.txt") (Join-Path $StageDir "README.txt") -Force

if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path $StageDir -DestinationPath $ZipPath -CompressionLevel Optimal

$mb = [math]::Round((Get-Item $ZipPath).Length / 1MB, 1)
Write-Host "`nPackaged: $ZipPath ($mb MB)" -ForegroundColor Green
Write-Host "Contents:" -ForegroundColor Green
Get-ChildItem $StageDir | ForEach-Object { "  {0,-38} {1,8:N0} KB" -f $_.Name, ($_.Length/1KB) }
