#!/usr/bin/env bash
# Build release binaries for Apple Silicon and Intel, then optionally lipo a universal binary.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
export PATH="${HOME}/.cargo/bin:${PATH}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust from https://rustup.rs and run: source \"\$HOME/.cargo/env\"" >&2
  exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin 2>/dev/null || true

echo "Building aarch64-apple-darwin (release)…"
cargo build --release --target aarch64-apple-darwin

echo "Building x86_64-apple-darwin (release)…"
cargo build --release --target x86_64-apple-darwin

ARM="target/aarch64-apple-darwin/release/rusty-jack"
INTEL="target/x86_64-apple-darwin/release/rusty-jack"
UNI="target/release/rusty-jack-universal"

mkdir -p target/release
lipo -create "$ARM" "$INTEL" -output "$UNI"

echo "Universal binary: $UNI"
file "$ARM" "$INTEL" "$UNI"
