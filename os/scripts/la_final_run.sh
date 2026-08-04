#!/bin/bash
set -euo pipefail

qemu_args=(
    -kernel "${WOS_KERNEL:-./kernel-la-final}"
    -m 8G
    -nographic
    -smp "${WOS_SMP:-8}"
    -drive "file=${WOS_SDCARD:-./sdcard-la.img},if=none,format=raw,id=x0"
    -device virtio-blk-pci,drive=x0
    -no-reboot
    -device virtio-net-pci,netdev=net0
    -netdev user,id=net0
    -rtc base=utc
)

wos_mode="${WOS_MODE:-auto}"
wos_smp="${WOS_SMP:-8}"
wos_cmdline="wos.mode=${wos_mode} wos.cpus=${wos_smp}"
[[ -n "${WOS_SHELL:-}" ]] && wos_cmdline+=" wos.shell=${WOS_SHELL}"
[[ -n "${WOS_SCRIPT:-}" ]] && wos_cmdline+=" wos.script=${WOS_SCRIPT}"
[[ -n "${WOS_ON_EXIT:-}" ]] && wos_cmdline+=" wos.on_exit=${WOS_ON_EXIT}"
[[ -n "${WOS_TTY:-}" ]] && wos_cmdline+=" wos.tty=${WOS_TTY}"
[[ -n "${WOS_LOG:-}" ]] && wos_cmdline+=" wos.log=${WOS_LOG}"
qemu_args+=(-append "$wos_cmdline")

# QEMU LoongArch direct ELF boot currently enters with argc/argv/envp cleared.
# Mirror bootargs into a validated early-RAM mailbox consumed by platform::boot.
wos_cmdline_file="$(mktemp -t wateros-la-bootargs.XXXXXX)"
trap 'rm -f "$wos_cmdline_file"' EXIT
printf 'WOSCMD1%s\0' "$wos_cmdline" > "$wos_cmdline_file"
qemu_args+=(-device "loader,file=${wos_cmdline_file},addr=0xa0000000,force-raw=on")

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

qemu-system-loongarch64 "${qemu_args[@]}"
