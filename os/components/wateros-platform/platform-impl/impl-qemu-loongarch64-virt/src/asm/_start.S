# QEMU virt 固件入口 shim。
# 固件 ABI: $r4 = argc, $r5 = argv, $r6 = envp/bootparams.
# arch boot ABI: $r4 = logical CPU id, $r5/$r6/$r7 = argc/argv/envp.
    .section .text.entry.platform
    .globl _start
    .align 2

_start:
    move $r7, $r6
    move $r6, $r5
    move $r5, $r4
    csrrd $r4, 0x20
    b __wateros_arch_boot
