#!/usr/bin/env bash
set -euo pipefail

crate_dir="$(cd "$(dirname "$0")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

dtc -I dts -O dtb -o "$tmp_dir/valid.dtb" \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts"
dtc -I dts -O dtb -o "$tmp_dir/invalid.dtb" \
    "$crate_dir/tests/fixtures/missing-uart-clock.dts"
dtc -I dts -O dtb -o "$tmp_dir/overlap.dtb" \
    "$crate_dir/tests/fixtures/overlapping-parent-map.dts"

cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- valid "$tmp_dir/valid.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/invalid.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/overlap.dtb"
