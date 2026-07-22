# LoongArch64 通用内核启动：使用规范化参数选择 per-CPU 栈，再进入 Rust。
# $r4 = logical CPU id, $r5/$r6 = platform boot arguments.
    .section .text.entry.arch
    .globl __wateros_arch_boot
    .equ STACK_PER_CPU_SHIFT, 16
    .equ STACK_PER_CPU, 1 << STACK_PER_CPU_SHIFT
    .equ MAX_CPUS, 8

__wateros_arch_boot:
    slli.d $t0, $r4, STACK_PER_CPU_SHIFT
    la.local $t1, __wateros_arch_boot_stacks
    add.d $sp, $t1, $t0
    li.d $t0, STACK_PER_CPU
    add.d $sp, $sp, $t0
    bl wateros_kernel_main

1:
    idle 0
    b 1b

    .section .bss.stack
    .globl __wateros_arch_boot_stacks
    .balign 4096
__wateros_arch_boot_stacks:
    .space STACK_PER_CPU * MAX_CPUS
