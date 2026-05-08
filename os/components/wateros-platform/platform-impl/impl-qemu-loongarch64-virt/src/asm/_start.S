# 内核入口：与 Rust `kernel_main` 约定 `$r3`（sp）指向 `boot_stack_top`（LP64 调用约定）。
    .section .text.entry
    .globl _start
    .align 2

_start:
    la.local $r3, boot_stack_top
    bl kernel_main

# `kernel_main` 不应返回；若返回则低功耗挂起，便于 QEMU 下观察异常退出。
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
