#!/usr/bin/env bash
# Explicit native filesystem microbenchmark. Never reads the normal Pi profile.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "Baseline source: $(git show 09195aadb6204c8605aed71dc490aa041765069d:src-tauri/src/storage.rs | shasum -a 256 | cut -d' ' -f1) (09195aadb6204c8605aed71dc490aa041765069d)"
echo "Current source: $(git rev-parse HEAD)"
echo "Pi: $(pi --version 2>/dev/null || echo unavailable)"
echo "Comparing baseline-equivalent discovery and current-style registry write in a disposable native test profile. Not UI input-to-paint or a baseline binary."
cargo test --manifest-path src-tauri/Cargo.toml dev_workspace::tests::performance_fixture_shape_and_native_scan_comparison -- --ignored --nocapture
