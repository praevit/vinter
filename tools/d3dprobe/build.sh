#!/usr/bin/env bash
# Rebuilds d3dprobe.exe and updates the copy embedded in vinter-core.
set -euo pipefail
cd "$(dirname "$0")"
rustup target add x86_64-pc-windows-msvc >/dev/null
cargo build --release
cp target/x86_64-pc-windows-msvc/release/d3dprobe.exe ../../crates/vinter-core/assets/d3dprobe.exe
shasum -a 256 ../../crates/vinter-core/assets/d3dprobe.exe
