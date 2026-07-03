#!/usr/bin/env bash
# Install the flowoss binary + sherpa-onnx libs to ~/.local/bin and enable
# the daemon as a systemd user service (starts on login, no sudo needed).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
UNIT_DIR="$HOME/.config/systemd/user"

if [ ! -x "$REPO_DIR/target/release/flowoss" ]; then
    echo "Build first: cargo build --release" >&2
    exit 1
fi

mkdir -p "$BIN_DIR" "$UNIT_DIR"
# The binary looks for the sherpa-onnx shared libs next to itself ($ORIGIN).
cp "$REPO_DIR/target/release/flowoss" "$BIN_DIR/"
cp "$REPO_DIR"/target/release/libsherpa-onnx-c-api.so \
   "$REPO_DIR"/target/release/libonnxruntime.so* "$BIN_DIR/" 2>/dev/null || true

cp "$REPO_DIR/packaging/flowoss-daemon.service" "$UNIT_DIR/"
systemctl --user daemon-reload
systemctl --user enable --now flowoss-daemon.service

echo "Installed. Check status with: systemctl --user status flowoss-daemon"
