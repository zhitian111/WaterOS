# RISC-V 通用内核启动：使用规范化参数选择 per-CPU 栈，再进入 Rust。
# a0 = logical CPU id, a1/a2 = platform boot arguments.
    .section .text.entry.arch
    .globl __wateros_arch_boot
    .equ STACK_PER_CPU_SHIFT, 16
    .equ STACK_PER_CPU, 1 << STACK_PER_CPU_SHIFT
    .equ MAX_CPUS, 8

__wateros_arch_boot:
    mv tp, a0
    slli t0, a0, STACK_PER_CPU_SHIFT
    la t1, __wateros_arch_boot_stacks
    add sp, t1, t0
    li t0, STACK_PER_CPU
    add sp, sp, t0
    call wateros_kernel_main

1:
    wfi
    j 1b

    .section .bss.stack
    .globl __wateros_arch_boot_stacks
    .balign 4096
__wateros_arch_boot_stacks:
    .space STACK_PER_CPU * MAX_CPUS
