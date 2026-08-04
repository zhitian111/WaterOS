#!/bin/bash
set -euo pipefail

qemu_args=(
    -machine virt
    -kernel "${WOS_KERNEL:-./kernel-rv-final}"
    -m 8G
    -nographic
    -smp "${WOS_SMP:-8}"
    -bios default
    -drive "file=${WOS_SDCARD:-./sdcard-rv-pub.img},if=none,format=raw,id=x0"
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
    -no-reboot
    -device virtio-net-device,netdev=net
    -netdev user,id=net
    -rtc base=utc
)

wos_mode="${WOS_MODE:-auto}"
wos_cmdline="wos.mode=${wos_mode}"
[[ -n "${WOS_SHELL:-}" ]] && wos_cmdline+=" wos.shell=${WOS_SHELL}"
[[ -n "${WOS_SCRIPT:-}" ]] && wos_cmdline+=" wos.script=${WOS_SCRIPT}"
[[ -n "${WOS_ON_EXIT:-}" ]] && wos_cmdline+=" wos.on_exit=${WOS_ON_EXIT}"
[[ -n "${WOS_TTY:-}" ]] && wos_cmdline+=" wos.tty=${WOS_TTY}"
[[ -n "${WOS_LOG:-}" ]] && wos_cmdline+=" wos.log=${WOS_LOG}"
qemu_args+=(-append "$wos_cmdline")

if [[ "${WOS_QEMU_SNAPSHOT:-0}" == "1" ||
      ( "$wos_mode" != "auto" && "${WOS_WRITE_DISK:-0}" != "1" ) ]]; then
    qemu_args+=(-snapshot)
fi

if [[ "${WOS_QEMU_GDB:-0}" == "1" || "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_args+=(-gdb "tcp:127.0.0.1:${WOS_QEMU_GDB_PORT:-1234}")
fi
if [[ "${WOS_QEMU_GDB_WAIT:-0}" == "1" ]]; then
    qemu_args+=(-S)
fi

exec qemu-system-riscv64 "${qemu_args[@]}"
