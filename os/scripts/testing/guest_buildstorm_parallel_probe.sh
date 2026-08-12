#!/bin/sh
# guest 侧 BuildStorm 探针：无需访问 registry，构造 Cargo/rustc 并发编译负载。

set -u

ROOT=/tmp/wateros-buildstorm-probe
MEMBERS=
CRATES=8

export PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/sbin:/usr/sbin
export HOME=/root
export RUSTUP_HOME=/root/.rustup
export CARGO_HOME=/root/.cargo
export RUSTUP_TOOLCHAIN=nightly-2026-05-28
export CARGO_NET_OFFLINE=true

mount -t proc proc /proc 2>/dev/null
mount -t sysfs sysfs /sys 2>/dev/null
mount -t devtmpfs devtmpfs /dev 2>/dev/null

rm -rf "$ROOT"
mkdir -p "$ROOT"

i=0
while [ "$i" -lt "$CRATES" ]; do
    name="probe_$i"
    dir="$ROOT/$name"
    mkdir -p "$dir/src"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n' "$name" \
        > "$dir/Cargo.toml"
    printf 'fn main() { let value = (0u64..10000).fold(%su64, |a, b| a.wrapping_add(b)); println!("%s:{}", value); }\n' \
        "$i" "$name" > "$dir/src/main.rs"
    if [ -z "$MEMBERS" ]; then
        MEMBERS="\"$name\""
    else
        MEMBERS="$MEMBERS, \"$name\""
    fi
    i=$((i + 1))
done

printf '[workspace]\nresolver = "2"\nmembers = [%s]\n' "$MEMBERS" > "$ROOT/Cargo.toml"

echo "BUILDSTORM_PROBE_BEGIN crates=$CRATES jobs=8"
T0=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
(
    cd "$ROOT" &&
        cargo build --workspace --offline -j8
)
RC=$?
T1=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)
ELAPSED=$(awk "BEGIN{printf \"%.2f\", (\"$T1\"+0)-(\"$T0\"+0)}" 2>/dev/null)

BUILT=0
i=0
while [ "$i" -lt "$CRATES" ]; do
    [ -x "$ROOT/target/debug/probe_$i" ] && BUILT=$((BUILT + 1))
    i=$((i + 1))
done

echo "BUILDSTORM_PROBE_END rc=$RC built=$BUILT elapsed_s=${ELAPSED:-0}"
[ "$RC" -eq 0 ] && [ "$BUILT" -eq "$CRATES" ]
