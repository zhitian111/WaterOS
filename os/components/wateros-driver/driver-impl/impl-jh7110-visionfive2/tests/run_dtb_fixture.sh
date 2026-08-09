#!/bin/sh
set -eu

crate_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture="$crate_dir/tests/fixtures/visionfive2-minimal.dts"
output=$(mktemp "${TMPDIR:-/tmp}/wateros-vf2-dtb.XXXXXX")
malformed_source=$(mktemp "${TMPDIR:-/tmp}/wateros-vf2-bad-dts.XXXXXX")
malformed_output=$(mktemp "${TMPDIR:-/tmp}/wateros-vf2-bad-dtb.XXXXXX")
trap 'rm -f "$output" "$malformed_source" "$malformed_output"' EXIT HUP INT TERM

dtc -I dts -O dtb -o "$output" "$fixture"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- "$output"

# Duplicate both required clock names. Discovery must reject the ambiguous
# mapping instead of selecting the first entry silently.
sed '0,/clock-names = "biu", "ciu";/s//clock-names = "biu", "biu";/' \
  "$fixture" > "$malformed_source"
dtc -I dts -O dtb -o "$malformed_output" "$malformed_source"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- \
  --expect-invalid "$malformed_output"

# A syntactically valid raw phandle with no provider node must be rejected.
sed '0,/<&syscrg 91>/s//<0xffffffff 91>/' "$fixture" > "$malformed_source"
dtc -I dts -O dtb -o "$malformed_output" "$malformed_source"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- \
  --expect-invalid "$malformed_output"

# Provider says #clock-cells=1, so omitting its argument is a truncated list.
sed '0,/<&syscrg 91>/s//<\&syscrg>/' "$fixture" > "$malformed_source"
dtc -f -I dts -O dtb -o "$malformed_output" "$malformed_source" 2>/dev/null
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- \
  --expect-invalid "$malformed_output"

# sysreg mask containing bits below its declared shift is ambiguous and unsafe.
sed '0,/0x7c000000/s//0x7c000001/' "$fixture" > "$malformed_source"
dtc -I dts -O dtb -o "$malformed_output" "$malformed_source"
cargo run --quiet --manifest-path "$crate_dir/Cargo.toml" --example inspect_dtb -- \
  --expect-invalid "$malformed_output"
