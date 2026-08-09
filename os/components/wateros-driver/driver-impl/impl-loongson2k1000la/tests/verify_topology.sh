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
sed '/cd-gpios/a\        broken-cd;' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/card-detect-conflict.dts"
dtc -I dts -O dtb -o "$tmp_dir/card-detect-conflict.dtb" \
    "$tmp_dir/card-detect-conflict.dts"
sed 's/dmas = <&apbdma1 0>/dmas = <\&apbdma1>/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/truncated-dma.dts"
dtc -I dts -O dtb -o "$tmp_dir/truncated-dma.dtb" \
    "$tmp_dir/truncated-dma.dts"
sed 's/cd-gpios = <&gpio0 22 1>;/non-removable;/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/non-removable.dts"
dtc -I dts -O dtb -o "$tmp_dir/non-removable.dtb" \
    "$tmp_dir/non-removable.dts"
sed '/clocks = <&clk 0>;/ { x; /apbdma/ { x; d; }; x; }; /apbdma1:/ { h; }' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/missing-dma-clock.dts"
dtc -I dts -O dtb -o "$tmp_dir/missing-dma-clock.dtb" \
    "$tmp_dir/missing-dma-clock.dts"
sed '/dma-controller@1fe00c10/,/#dma-cells/ s/0x0 0x8/0x0 0x4/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/short-dma-mmio.dts"
dtc -I dts -O dtb -o "$tmp_dir/short-dma-mmio.dtb" \
    "$tmp_dir/short-dma-mmio.dts"

cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- valid "$tmp_dir/valid.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/invalid.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/overlap.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/card-detect-conflict.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/truncated-dma.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- non-removable "$tmp_dir/non-removable.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/missing-dma-clock.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/short-dma-mmio.dtb"
