#!/bin/bash
set -euo pipefail

qemu-system-riscv64 -machine virt \
    -kernel "${WOS_KERNEL:-./kernel-rv-final}" -m 8G -nographic -smp 8 -bios default \
    -drive file="${WOS_SDCARD:-./sdcard-rv-pub.img}",if=none,format=raw,id=x0 \
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot \
    -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc
