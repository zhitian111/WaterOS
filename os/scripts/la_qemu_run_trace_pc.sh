#!/bin/bash
os_file="./kernel-la"
fs="./sdcard-la.img"

qemu-system-loongarch64 -kernel $os_file \
                        -m 1G \
                        -nographic \
                        -smp 1 \
                        -drive file=$fs,if=none,format=raw,id=x0 \
                        -device virtio-blk-pci,drive=x0 \
                        -no-reboot \
                        -device virtio-net-pci,netdev=net0 \
                        -netdev user,id=net0 \
                        -rtc base=utc \
                        -d exec,nochain
