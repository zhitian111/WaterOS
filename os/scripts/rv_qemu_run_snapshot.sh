#!/bin/bash
# RISC-V QEMU：qcow2 overlay 写盘，backing 默认 test_case 只读镜像，不改动 os/sdcard-rv.img。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

os_file="${WOS_KERNEL:-./kernel-rv}"
backing="${WOS_SDCARD_BACKING:-../test_case/sdcard-rv.img}"
if [[ ! -f "$backing" ]]; then
    backing="./sdcard-rv.img"
fi
backing="$(readlink -f "$backing")"
overlay_dir="./tem"
snapshot_id="${WOS_SNAPSHOT_ID:-default}"
overlay="${overlay_dir}/sdcard-rv.${snapshot_id}.overlay.qcow2"

mkdir -p "$overlay_dir"
rm -f "$overlay"
qemu-img create -f qcow2 -b "$backing" -F raw "$overlay" >/dev/null
echo "[rv_qemu_run_snapshot] kernel=$os_file backing=$backing overlay=$overlay id=$snapshot_id" >&2

"$SCRIPT_DIR/qemu_exec_with_taskset.sh" qemu-system-riscv64 -machine virt \
    -kernel "$os_file" \
    -m 1G \
    -nographic \
    -smp 1 \
    -bios default \
    -drive file="$overlay",if=none,format=qcow2,id=x0 \
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
    -no-reboot \
    -device virtio-net-device,netdev=net \
    -netdev user,id=net \
    -rtc base=utc
