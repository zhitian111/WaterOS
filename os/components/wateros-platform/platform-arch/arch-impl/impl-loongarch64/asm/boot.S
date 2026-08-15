# LoongArch64 通用内核启动：使用规范化参数选择 per-CPU 栈，再进入 Rust。
# $r4 = logical CPU id, $r5/$r6/$r7 = platform boot arguments.
    .section .text.entry.arch
    .globl __wateros_arch_boot
    .equ STACK_PER_CPU_SHIFT, 16
    .equ STACK_PER_CPU, 1 << STACK_PER_CPU_SHIFT
    .equ MAX_CPUS, {max_cpus}

__wateros_arch_boot:
    slli.d $t0, $r4, STACK_PER_CPU_SHIFT
    la.abs $t1, __wateros_arch_boot_stacks
    la.abs $t2, __wateros_kernel_high_base
    ld.d $t2, $t2, 0
    or $t1, $t1, $t2
    add.d $sp, $t1, $t0
    li.d $t0, STACK_PER_CPU
    add.d $sp, $sp, $t0
    bl __wateros_platform_boot_marker
    bl wateros_kernel_main

1:
    idle 0
    b 1b

    .section .bss.stack
    .globl __wateros_arch_boot_stacks
    .balign 4096
__wateros_arch_boot_stacks:
    .space STACK_PER_CPU * MAX_CPUS

    .section .data.high_base
    .weak __wateros_kernel_high_base
    .balign 8
__wateros_kernel_high_base:
    .dword 0

    .section .text.entry.arch
    .weak __wateros_platform_boot_marker
    .type __wateros_platform_boot_marker, @function
__wateros_platform_boot_marker:
    jr $ra
