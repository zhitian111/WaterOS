#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

qemu_args=(
    -kernel "${WOS_KERNEL:-./kernel-la-pre}"
    -m 1G
    -nographic
    -smp "${WOS_SMP:-8}"
    -drive "file=${WOS_SDCARD:-./sdcard-la.img},if=none,format=raw,id=x0"
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
