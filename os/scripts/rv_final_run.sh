#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QEMU_MEM="${WOS_QEMU_MEM:-8G}"

qemu_args=(
    -machine virt
    -kernel "${WOS_KERNEL:-./kernel-rv-final}"
    -m "${QEMU_MEM}"
    -nographic
    -smp "${WOS_SMP:-8}"
    -bios default
    -drive "file=${WOS_SDCARD:-./sdcard-rv-pub.img},if=none,format=raw,id=x0${WOS_QEMU_IMAGE_DRIVE_OPTIONS:+,${WOS_QEMU_IMAGE_DRIVE_OPTIONS}}"
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
    -no-reboot
    -device virtio-net-device,netdev=net
    -netdev user,id=net
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

"$SCRIPT_DIR/qemu_exec_with_taskset.sh" qemu-system-riscv64 "${qemu_args[@]}"
