# 内核入口：固件（UEFI/GRUB 等）以 LP64 调用约定交接，传递
#   $r4 = argc
#   $r5 = argv
#   $r6 = envp (或 bootparams，部分固件通过此处传递 FDT 指针)
# 内核函数签名：kernel_main(argc, argv, envp)
# sp 指向 `boot_stack_top`。
    .section .text.entry
    .globl _start
    .align 2

_start:
    la.local $r3, boot_stack_top
    # $r4/$r5/$r6 已经是 kernel_main(argc, argv, envp) 的 LP64 调用约定参数
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
