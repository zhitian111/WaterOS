#!/bin/bash
# 保留的 RISC-V64 诊断入口：启用 QEMU 指令与中断日志并写入 qemu.log。
# 日志量很大，只适合短时间诊断。
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
