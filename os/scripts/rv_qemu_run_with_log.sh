#!/bin/bash
os_file="./kernel-rv"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fs="./sdcard-rv.img"

"$SCRIPT_DIR/qemu_exec_with_taskset.sh" qemu-system-riscv64 -machine virt \
                    -kernel $os_file \
                    -m 1G \
                    -nographic \
                    -smp 1 \
                    -bios default \
                    -drive file=$fs,if=none,format=raw,id=x0 \
                    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
                    -no-reboot \
                    -device virtio-net-device,netdev=net \
                    -netdev user,id=net \
                    -rtc base=utc \
                    -d int,cpu,in_asm \
                    -D qemu.log
