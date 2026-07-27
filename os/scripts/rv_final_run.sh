#!/bin/bash
set -euo pipefail

qemu_debug_args=()
if [[ "${WOS_QEMU_GDB:-0}" == "1" || "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_debug_args=(-gdb "tcp:127.0.0.1:${WOS_QEMU_GDB_PORT:-1234}")
fi
if [[ "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_debug_args+=(-S)
fi

qemu-system-riscv64 -machine virt \
    -kernel "${WOS_KERNEL:-./kernel-rv-final}" -m 8G -nographic -smp 8 -bios default \
    -drive file="${WOS_SDCARD:-./sdcard-rv-pub.img}",if=none,format=raw,id=x0 \
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot \
    -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc \
    "${qemu_debug_args[@]}"
