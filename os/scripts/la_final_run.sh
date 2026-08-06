#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QEMU_MEM="${WOS_QEMU_MEM:-8G}"
# 本机 i9-13980HX 的 P-core 为逻辑 CPU 0-15；默认避免落到 E-core。
WOS_TASKSET_CPUS="${WOS_TASKSET_CPUS:-0,2,4,6,8,10,12,14}"
export WOS_TASKSET_CPUS

qemu_args=(
    -kernel "${WOS_KERNEL:-./kernel-la-final}"
    -m "${QEMU_MEM}"
    -nographic
    -smp "${WOS_SMP:-8}"
    -drive "file=${WOS_SDCARD:-./sdcard-la-pub.img},if=none,format=raw,id=x0${WOS_QEMU_IMAGE_DRIVE_OPTIONS:+,${WOS_QEMU_IMAGE_DRIVE_OPTIONS}}"
    -device virtio-blk-pci,drive=x0
    -no-reboot
    -device virtio-net-pci,netdev=net0
    -netdev user,id=net0
    -rtc base=utc
)

if [[ "${WOS_QEMU_SNAPSHOT:-0}" == "1" ]]; then
    qemu_args+=(-snapshot)
fi

if [[ "${WOS_QEMU_GDB:-0}" == "1" || "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_args+=(-gdb "tcp:127.0.0.1:${WOS_QEMU_GDB_PORT:-1234}")
fi
if [[ "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_args+=(-S)
fi

"$SCRIPT_DIR/qemu_exec_with_taskset.sh" qemu-system-loongarch64 "${qemu_args[@]}"
