# RISC-V 通用内核启动：使用规范化参数选择 per-CPU 栈，再进入 Rust。
# a0 = logical CPU id, a1/a2 = platform boot arguments.
    .section .text.entry.arch
    .globl __wateros_arch_boot
    .equ STACK_PER_CPU_SHIFT, 16
    .equ STACK_PER_CPU, 1 << STACK_PER_CPU_SHIFT
    .equ MAX_CPUS, {max_cpus}

__wateros_arch_boot:
    # WaterOS 用户态使用 lp64d。内核 trap 路径会在进入 Rust 前保存完整
    # FPU 状态，因此所有 hart 从启动起都必须允许 S/U 态执行 F/D 指令。
    csrr t0, sstatus
    li t1, 3 << 13
    or t0, t0, t1
    csrw sstatus, t0
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
