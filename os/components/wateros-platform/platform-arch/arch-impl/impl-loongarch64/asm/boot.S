# LoongArch64 通用内核启动：使用规范化参数选择 per-CPU 栈，再进入 Rust。
# $r4 = logical CPU id, $r5/$r6/$r7 = platform boot arguments.
    .section .text.entry.arch
    .globl __wateros_arch_boot
    .equ STACK_PER_CPU_SHIFT, 16
    .equ STACK_PER_CPU, 1 << STACK_PER_CPU_SHIFT
    .equ MAX_CPUS, {max_cpus}

__wateros_arch_boot:
    move $t3, $a0
    slli.d $t0, $t3, STACK_PER_CPU_SHIFT
    la.abs $t1, __wateros_arch_boot_stacks
    bl __wateros_get_kernel_high_base
    or $t1, $t1, $a0
    add.d $sp, $t1, $t0
    li.d $t0, STACK_PER_CPU
    add.d $sp, $sp, $t0
    bl __wateros_platform_boot_marker
    move $a0, $t3
    bl wateros_kernel_main

1:
    idle 0
    b 1b

    .section .bss.stack
    .globl __wateros_arch_boot_stacks
    .balign 4096
__wateros_arch_boot_stacks:
    .space STACK_PER_CPU * MAX_CPUS

    .section .text.entry.arch
    .weak __wateros_platform_boot_marker
    .type __wateros_platform_boot_marker, @function
__wateros_platform_boot_marker:
    jr $ra

    .weak __wateros_get_kernel_high_base
    .type __wateros_get_kernel_high_base, @function
__wateros_get_kernel_high_base:
    move $a0, $zero
    jr $ra
