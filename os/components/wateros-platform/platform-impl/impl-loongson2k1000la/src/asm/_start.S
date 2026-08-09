# 2K1000LA legacy LoongArch boot ABI: a0/a1/a2 are preserved as argc/argv/envp.
    .section .text.entry.platform
    .globl _start
    .align 2
_start:
    move $r7, $r6
    move $r6, $r5
    move $r5, $r4
    csrrd $r4, 0x20
    b __wateros_arch_boot
