# switch.S — 协作式上下文切换，与 `../src/task.rs` 中 `Riscv64ArchTaskContext` 布局一致。
# a0 = 旧上下文指针, a1 = 新上下文指针；保存/恢复 ra、sp 与 s0–s11。
    .altmacro
    .macro SAVE_SN n
        sd s\n, (\n+2)*8(a0)
    .endm
    .macro LOAD_SN n
        ld s\n, (\n+2)*8(a1)
    .endm

    .section .text
    .globl __switch
__switch:
    sd sp, 8(a0)
    sd ra, 0(a0)
    .set n, 0
    .rept 12
        SAVE_SN %n
        .set n, n + 1
    .endr

    ld ra, 0(a1)
    .set n, 0
    .rept 12
        LOAD_SN %n
        .set n, n + 1
    .endr
    ld sp, 8(a1)
    ret

    .globl __arch_task_entry
__arch_task_entry:
    mv a0, s0
    tail __wateros_arch_task_entry_trampoline

    .globl __arch_user_task_entry
__arch_user_task_entry:
    tail __wateros_arch_user_task_entry_trampoline

    .globl __wateros_arch_restore_user_task
__wateros_arch_restore_user_task:
    ld t0, 32*8(a0)
    csrw sstatus, t0
    ld t0, 33*8(a0)
    csrw sepc, t0

    # a1 holds the kernel stack top that user-mode traps should switch to.
    mv sp, a1
    ld t0, 2*8(a0)
    csrw sscratch, t0

    ld x1,  1*8(a0)
    ld x3,  3*8(a0)
    ld x4,  4*8(a0)
    ld x5,  5*8(a0)
    ld x6,  6*8(a0)
    ld x7,  7*8(a0)
    ld x8,  8*8(a0)
    ld x9,  9*8(a0)
    ld x11, 11*8(a0)
    ld x12, 12*8(a0)
    ld x13, 13*8(a0)
    ld x14, 14*8(a0)
    ld x15, 15*8(a0)
    ld x16, 16*8(a0)
    ld x17, 17*8(a0)
    ld x18, 18*8(a0)
    ld x19, 19*8(a0)
    ld x20, 20*8(a0)
    ld x21, 21*8(a0)
    ld x22, 22*8(a0)
    ld x23, 23*8(a0)
    ld x24, 24*8(a0)
    ld x25, 25*8(a0)
    ld x26, 26*8(a0)
    ld x27, 27*8(a0)
    ld x28, 28*8(a0)
    ld x29, 29*8(a0)
    ld x30, 30*8(a0)
    ld x31, 31*8(a0)

    ld x10, 10*8(a0)
    csrrw sp, sscratch, sp
    sret
