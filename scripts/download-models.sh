#!/usr/bin/env bash
# Download the default FlowOSS models (explicit user action per PRD §14/§15):
#   - NVIDIA Parakeet TDT 0.6b v2 int8 (English), ~700MB, CC-BY-4.0
#   - Silero VAD, ~2MB (check upstream license: https://github.com/snakers4/silero-vad)
# Both are served from sherpa-onnx release assets.
set -euo pipefail

MODELS_DIR="${FLOWOSS_MODELS_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/flowoss/models}"
STT_MODEL="sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8"
BASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"

mkdir -p "$MODELS_DIR"
cd "$MODELS_DIR"

if [ ! -f silero_vad.onnx ]; then
    echo "Downloading Silero VAD..."
    curl -SL -o silero_vad.onnx "$BASE_URL/silero_vad.onnx"
else
    echo "Silero VAD already present."
fi

if [ ! -d "$STT_MODEL" ]; then
    echo "Downloading $STT_MODEL (~700MB)..."
    curl -SL -o "$STT_MODEL.tar.bz2" "$BASE_URL/$STT_MODEL.tar.bz2"
    echo "Extracting..."
    tar xjf "$STT_MODEL.tar.bz2"
    rm "$STT_MODEL.tar.bz2"
else
    echo "$STT_MODEL already present."
fi

echo "Models ready in $MODELS_DIR"
