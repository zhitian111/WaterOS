#!/bin/bash
# LoongArch QEMU：qcow2 overlay 写盘，backing 默认 test_case 只读镜像，不改动 os/sdcard-la.img。
set -euo pipefail

backing="${WOS_SDCARD_BACKING:-../test_case/sdcard-la.img}"
if [[ ! -f "$backing" ]]; then
    backing="./sdcard-la.img"
fi
backing="$(readlink -f "$backing")"
overlay_dir="./tem"
snapshot_id="${WOS_SNAPSHOT_ID:-default}"
overlay="${overlay_dir}/sdcard-la.${snapshot_id}.overlay.qcow2"

mkdir -p "$overlay_dir"
rm -f "$overlay"
qemu-img create -f qcow2 -b "$backing" -F raw "$overlay" >/dev/null
os_file="${WOS_KERNEL:-./kernel-la}"
echo "[la_qemu_run_snapshot] kernel=$os_file backing=$backing overlay=$overlay id=$snapshot_id" >&2

qemu-system-loongarch64 -kernel "$os_file" -m 1G -nographic -smp 1 \
    -drive file="$overlay",if=none,format=qcow2,id=x0 \
    -device virtio-blk-pci,drive=x0 -no-reboot \
    -device virtio-net-pci,netdev=net0 \
    -netdev user,id=net0 -rtc base=utc
