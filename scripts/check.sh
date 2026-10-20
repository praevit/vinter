#!/usr/bin/env bash
# Runs every check CI runs. Usage: scripts/check.sh
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd app && npm ci && npm run lint && npm run build)

# `cargo test` regenerates the TS bindings; fail if they were stale.
git diff --exit-code -- app/src/api/bindings.ts
