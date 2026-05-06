    .section .text.reset
    .globl qemu_loongarch64_reset_vector
    .align 2

qemu_loongarch64_reset_vector:
    ori $r12, $r0, 0x900
    slli.d $r12, $r12, 20
    jirl $r0, $r12, 0

    .section .text.entry
    .globl _start
    .align 2

_start:
    la.local $r3, boot_stack_top
    bl kernel_main

1:
    idle 0
    b 1b

    .section .bss.stack
    .align 12
    .globl boot_stack_lower_bound
boot_stack_lower_bound:
    .space 4096 * 16
    .globl boot_stack_top
boot_stack_top:
