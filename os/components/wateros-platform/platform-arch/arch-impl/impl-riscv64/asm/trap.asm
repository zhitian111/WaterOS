    .section .text.trampoline
    .globl __alltraps
    .align 2

# 方案A：入口汇编只做“上下文快照 -> 栈上放置”，不做分发、不做返回链路。
# Rust 侧用固定符号 `trap_entry_rust(cx_ptr)` 接管。
#
# TrapContext 布局（与 rust::TrapContext #[repr(C)] 保持一致）：
# - x[0..31] : 32 * 8
# - sstatus  : 32 * 8
# - sepc     : 33 * 8
# - scause   : 34 * 8
# - stval    : 35 * 8
# 总大小：36 * 8 = 288 字节
#
# a0 = TrapContext*  (交给 trap_entry_rust)

__alltraps:
    # 备份当前 sp（用于写入 TrapContext.x[2]）
    mv t0, sp

    # 在当前 sp 下方分配 TrapContext 空间
    addi sp, sp, -288

    # 保存通用寄存器（x0..x31）
    sd x0,  0*8(sp)
    sd x1,  1*8(sp)
    sd t0,  2*8(sp)   # x2 = 进入 trap 时的 sp
    sd x3,  3*8(sp)
    sd x4,  4*8(sp)
    sd x5,  5*8(sp)
    sd x6,  6*8(sp)
    sd x7,  7*8(sp)
    sd x8,  8*8(sp)
    sd x9,  9*8(sp)
    sd x10, 10*8(sp)
    sd x11, 11*8(sp)
    sd x12, 12*8(sp)
    sd x13, 13*8(sp)
    sd x14, 14*8(sp)
    sd x15, 15*8(sp)
    sd x16, 16*8(sp)
    sd x17, 17*8(sp)
    sd x18, 18*8(sp)
    sd x19, 19*8(sp)
    sd x20, 20*8(sp)
    sd x21, 21*8(sp)
    sd x22, 22*8(sp)
    sd x23, 23*8(sp)
    sd x24, 24*8(sp)
    sd x25, 25*8(sp)
    sd x26, 26*8(sp)
    sd x27, 27*8(sp)
    sd x28, 28*8(sp)
    sd x29, 29*8(sp)
    sd x30, 30*8(sp)
    sd x31, 31*8(sp)

    # 保存控制寄存器
    csrr t1, sstatus
    sd t1, 32*8(sp)
    csrr t1, sepc
    sd t1, 33*8(sp)
    csrr t1, scause
    sd t1, 34*8(sp)
    csrr t1, stval
    sd t1, 35*8(sp)

    # a0 = cx_ptr
    mv a0, sp
    # 调用 Rust 入口，返回后从当前 sp 上的 TrapContext 恢复现场
    call trap_entry_rust

    # 恢复控制寄存器
    ld t0, 32*8(sp)
    csrw sstatus, t0
    ld t0, 33*8(sp)
    csrw sepc, t0

    # 先把原始 sp 暂存到 sscratch，再恢复通用寄存器
    ld t0, 2*8(sp)
    csrw sscratch, t0

    ld x1,  1*8(sp)
    ld x3,  3*8(sp)
    ld x4,  4*8(sp)
    ld x5,  5*8(sp)
    ld x6,  6*8(sp)
    ld x7,  7*8(sp)
    ld x8,  8*8(sp)
    ld x9,  9*8(sp)
    ld x10, 10*8(sp)
    ld x11, 11*8(sp)
    ld x12, 12*8(sp)
    ld x13, 13*8(sp)
    ld x14, 14*8(sp)
    ld x15, 15*8(sp)
    ld x16, 16*8(sp)
    ld x17, 17*8(sp)
    ld x18, 18*8(sp)
    ld x19, 19*8(sp)
    ld x20, 20*8(sp)
    ld x21, 21*8(sp)
    ld x22, 22*8(sp)
    ld x23, 23*8(sp)
    ld x24, 24*8(sp)
    ld x25, 25*8(sp)
    ld x26, 26*8(sp)
    ld x27, 27*8(sp)
    ld x28, 28*8(sp)
    ld x29, 29*8(sp)
    ld x30, 30*8(sp)
    ld x31, 31*8(sp)

    csrr sp, sscratch
    sret
