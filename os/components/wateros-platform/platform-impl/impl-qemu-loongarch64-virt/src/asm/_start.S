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
