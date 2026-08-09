#!/bin/sh
set -eu

crate_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture="$crate_dir/tests/fixtures/visionfive2-minimal.dts"
output=$(mktemp "${TMPDIR:-/tmp}/wateros-vf2-dtb.XXXXXX")
trap 'rm -f "$output"' EXIT HUP INT TERM

dtc -I dts -O dtb -o "$output" "$fixture"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- "$output"
