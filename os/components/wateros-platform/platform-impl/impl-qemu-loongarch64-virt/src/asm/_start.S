# 内核入口：固件（UEFI/GRUB 等）以 LP64 调用约定交接，传递
#   $r4 = argc
#   $r5 = argv
#   $r6 = envp (或 bootparams，部分固件通过此处传递 FDT 指针)
# 内核函数签名：kernel_main(argc, argv, envp)
    .section .text.entry
    .globl _start
    .equ STACK_PER_CPU, 4096 * 16
    .align 2

_start:
    csrrd $t0, 0x10             # $t0 = cpuid
    slli.d $t1, $t0, 14        # $t1 = cpuid * 16384
    la.local $t2, cpu_stacks
    add.d $t2, $t2, $t1         # $t2 = 本 CPU 栈区基址
    li.d  $t0, STACK_PER_CPU
    add.d $sp, $t2, $t0         # sp = 栈基址 + 16KB → 栈顶
    # $r4/$r5/$r6 已经是 kernel_main(argc, argv, envp)
    bl kernel_main

# `kernel_main` 不应返回；若返回则低功耗挂起，便于 QEMU 下观察异常退出。
1:
    idle 0
    b 1b

    .section .bss.stack
    .global cpu_stacks
    .balign 4096
cpu_stacks:
    .space 4096 * 16 * 8        # 8 CPUs × 16KB = 128KB
