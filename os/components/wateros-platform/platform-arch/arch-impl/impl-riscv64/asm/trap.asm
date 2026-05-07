# trap.asm — WaterOS RISC-V S-mode 异常/中断入口（方案A）。
# 与 `../src/trap.rs` 中 `TrapContext`、`trap_entry_rust` 成对维护；布局变更须同步 Rust `#[repr(C)]`。
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
#
# 约定：
# - 从用户态进入 trap 前，sscratch 保存“当前任务下次 trap 应切回的内核栈顶”
# - 从用户态进入时，入口先用 csrrw 把 sp 切到内核栈，再在内核栈上分配 TrapContext
# - 返回用户态时，用 csrrw 把 sp 切回用户栈，同时把新的内核栈顶重新留在 sscratch

__alltraps:
    csrr t1, sstatus
    andi t2, t1, 1 << 8
    bnez t2, .Ltrap_from_kernel

    # User -> kernel: swap to the task's kernel stack first.
    csrrw sp, sscratch, sp
    addi sp, sp, -288
    csrr t0, sscratch
    j .Lsave_context

.Ltrap_from_kernel:
    mv t0, sp
    addi sp, sp, -288

.Lsave_context:

    # 保存通用寄存器（x0..x31）
    sd x0,  0*8(sp)
    sd x1,  1*8(sp)
    sd t0,  2*8(sp)   # x2 = 进入 trap 时的原始 sp（用户栈或内核栈）
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
    sd t1, 32*8(sp)
    csrr t0, sepc
    sd t0, 33*8(sp)
    csrr t0, scause
    sd t0, 34*8(sp)
    csrr t0, stval
    sd t0, 35*8(sp)

    # a0 = cx_ptr
    mv a0, sp
    # 调用 Rust 入口，返回后从当前 sp 上的 TrapContext 恢复现场
    call trap_entry_rust

    # 恢复控制寄存器
    ld t0, 32*8(sp)
    andi t1, t0, 1 << 8
    csrw sstatus, t0
    ld t0, 33*8(sp)
    csrw sepc, t0

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

    bnez t1, .Ltrap_return_kernel

    addi sp, sp, 288
    csrrw sp, sscratch, sp
    sret

.Ltrap_return_kernel:
    ld t0, 2*8(sp)
    mv sp, t0
    sret
