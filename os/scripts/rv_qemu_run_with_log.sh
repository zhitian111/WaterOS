#!/bin/bash
mem="256M"
os_file="./kernel-rv"
fs="./sdcard-rv.img"

qemu-system-riscv64 -machine virt \
                    -nographic \
                    -kernel $os_file \
                    -serial mon:stdio \
                    -bios default \
                    -no-reboot \
                    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
                    -m $mem \
                    -rtc base=utc \
                    -drive file=$fs,if=none,format=raw,id=x0 \
                    -d int,cpu,in_asm \
                    -D qemu.log
