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
sed '/clocks = <&clk 12>;/ { x; /apbdma/ { x; d; }; x; }; /apbdma1:/ { h; }' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/missing-dma-clock.dts"
dtc -I dts -O dtb -o "$tmp_dir/missing-dma-clock.dtb" \
    "$tmp_dir/missing-dma-clock.dts"
sed '/dma-controller@1fe00c10/,/#dma-cells/ s/0x0 0x8/0x0 0x4/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/short-dma-mmio.dts"
dtc -I dts -O dtb -o "$tmp_dir/short-dma-mmio.dtb" \
    "$tmp_dir/short-dma-mmio.dts"
sed '/dma-controller@1fe00c10/,/#dma-cells/ s/0x1fe00c10/0x1fe00c12/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/unaligned-dma-mmio.dts"
dtc -I dts -O dtb -o "$tmp_dir/unaligned-dma-mmio.dtb" \
    "$tmp_dir/unaligned-dma-mmio.dts"
sed '/clock-controller@1fe00480/,/#clock-cells/ s/0x0 0x58/0x0 0x2/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/short-clock-mmio.dts"
dtc -I dts -O dtb -o "$tmp_dir/short-clock-mmio.dtb" \
    "$tmp_dir/short-clock-mmio.dts"
sed '/regulator-vmmc/a\        regulator-always-on = <1>;' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/malformed-regulator-flag.dts"
dtc -I dts -O dtb -o "$tmp_dir/malformed-regulator-flag.dtb" \
    "$tmp_dir/malformed-regulator-flag.dts"
sed '/mmc@1fe2c000/,/vmmc-supply/ s/clocks = <&clk 12>/clocks = <\&clk 0>/' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/wrong-mmc-clock-id.dts"
dtc -I dts -O dtb -o "$tmp_dir/wrong-mmc-clock-id.dtb" \
    "$tmp_dir/wrong-mmc-clock-id.dts"
sed '/clock-controller@1fe00480/,/clock-names/ { /clocks = <&ref_100m>;/d; }' \
    "$crate_dir/tests/fixtures/loongson2k1000la-topology.dts" \
    > "$tmp_dir/missing-clock-reference.dts"
dtc -I dts -O dtb -o "$tmp_dir/missing-clock-reference.dtb" \
    "$tmp_dir/missing-clock-reference.dts"

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
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/unaligned-dma-mmio.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/short-clock-mmio.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/malformed-regulator-flag.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/wrong-mmc-clock-id.dtb"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" \
    --example verify_topology -- invalid "$tmp_dir/missing-clock-reference.dtb"
