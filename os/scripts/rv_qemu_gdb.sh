#!/bin/bash
os_file="./kernel-rv"
fs="./sdcard-rv.img"

qemu-system-riscv64 -machine virt \
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
                    -s -S
