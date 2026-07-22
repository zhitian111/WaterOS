#!/bin/bash
set -euo pipefail

qemu-system-loongarch64 -kernel "${WOS_KERNEL:-./kernel-la-final}" -m 8G -nographic -smp 8 \
    -drive file=./sdcard-la.img,if=none,format=raw,id=x0 -device virtio-blk-pci,drive=x0 -no-reboot \
    -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc
