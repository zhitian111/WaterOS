#!/usr/bin/env bash
set -euo pipefail

crate_dir="$(cd "$(dirname "$0")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

dtc -I dts -O dtb \
    -o "$tmp_dir/official.dtb" \
    "$crate_dir/tests/fixtures/official-memory-layout.dts"
dtc -I dts -O dtb \
    -o "$tmp_dir/missing.dtb" \
    "$crate_dir/tests/fixtures/no-kernel-memory.dts"

cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_memory_dtb -- official "$tmp_dir/official.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_memory_dtb -- missing "$tmp_dir/missing.dtb"
