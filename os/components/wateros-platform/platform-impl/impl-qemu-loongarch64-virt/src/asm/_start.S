# 内核入口：固件（UEFI/GRUB 等）以 LP64 调用约定交接，传递
#   $r4 = argc（通常已有）
#   $r5 = argv（通常已有）
#   $r6 = envp/bootparams（可选，后续 DTB 传递约定可能走此寄存器）
# 移交 Rust `kernel_main`；sp 指向 `boot_stack_top`。
    .section .text.entry
    .globl _start
    .align 2

_start:
    la.local $r3, boot_stack_top
    move $r4, $r4
    move $r5, $r5
    move $r6, $r6
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