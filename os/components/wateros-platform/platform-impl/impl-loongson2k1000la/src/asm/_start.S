# 2K1000LA unified ABI: a0/a1/a2 are the UEFI flag/cmdline/system-table values.
    .section .text.entry.platform
    .globl _start
    .align 2
_start:
    move $r7, $r6
    move $r6, $r5
    move $r5, $r4
    csrrd $r4, 0x20
    b __wateros_arch_boot
