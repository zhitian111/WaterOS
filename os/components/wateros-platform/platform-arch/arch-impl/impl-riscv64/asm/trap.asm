# trap.asm — WaterOS RISC-V S-mode 异常/中断入口（方案A）。
# 与 `../src/trap.rs` 中 `TrapContext`、`trap_entry_rust` 成对维护；布局变更须同步 Rust `#[repr(C)]`。
    .section .text.trampoline
    .globl __alltraps, __wateros_riscv_restore_user_from_frame, gdb_point
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
# - return_address_space_token : 36 * 8
# 总大小：37 * 8 = 296 字节
#
# a0 = TrapContext*  (交给 trap_entry_rust)
#
# 约定：
# - 从用户态进入 trap 前，sscratch 指向 supervisor-only trampoline frame。
# - trampoline frame 的 37*8 槽保存当前任务内核栈顶，供入口切到内核栈。
# - 返回用户态时，sscratch 重新指向 trampoline frame，用户 sp 从 TrapContext 恢复。

__alltraps:
    # User mode enters with sscratch = __wateros_riscv_return_frame.
    # Kernel mode keeps sscratch = 0.  The initial csrrw preserves user x5/t0
    # in sscratch and gives us a scratch frame pointer without clobbering any
    # other user GPR before the snapshot.
    csrrw t0, sscratch, t0
    beqz t0, .Ltrap_from_kernel
gdb_point_0:
    # Save the full user GPR set into the trampoline frame while still running
    # on the user page table.  The frame is supervisor-only but writable by S
    # mode, and is the only data page needed before switching to kernel satp.
    sd x0,  0*8(t0)
    sd x1,  1*8(t0)
    sd sp,  2*8(t0)
    sd x3,  3*8(t0)
    sd x4,  4*8(t0)
    sd x6,  6*8(t0)
    csrr t1, sscratch
    sd t1, 5*8(t0)
    # After x5/t0 is saved, clear sscratch before any further memory access.
    # Otherwise a nested S-mode fault in this save path would re-enter with
    # sscratch still holding the user t0 value and treat it as a frame pointer.
    csrw sscratch, x0
    sd x7,  7*8(t0)
    sd x8,  8*8(t0)
    sd x9,  9*8(t0)
    sd x10, 10*8(t0)
    sd x11, 11*8(t0)
    sd x12, 12*8(t0)
    sd x13, 13*8(t0)
    sd x14, 14*8(t0)
    sd x15, 15*8(t0)
    sd x16, 16*8(t0)
    sd x17, 17*8(t0)
    sd x18, 18*8(t0)
    sd x19, 19*8(t0)
    sd x20, 20*8(t0)
    sd x21, 21*8(t0)
    sd x22, 22*8(t0)
    sd x23, 23*8(t0)
    sd x24, 24*8(t0)
    sd x25, 25*8(t0)
    sd x26, 26*8(t0)
    sd x27, 27*8(t0)
    sd x28, 28*8(t0)
    sd x29, 29*8(t0)
    sd x30, 30*8(t0)
    sd x31, 31*8(t0)

    csrr t1, sstatus
    sd t1, 32*8(t0)
    csrr t1, sepc
    sd t1, 33*8(t0)
    csrr t1, scause
    sd t1, 34*8(t0)
    csrr t1, stval
    sd t1, 35*8(t0)
    csrr t1, satp
    sd t1, 36*8(t0)

    ld sp, 37*8(t0)
    addi sp, sp, -296
    la t1, __wateros_riscv_kernel_satp
    ld t1, 0(t1)
    csrw satp, t1
    sfence.vma x0, x0

    mv t1, t0
    mv t2, sp
    li t3, 37
.Lcopy_user_frame_to_kernel_stack:
    ld t4, 0(t1)
    sd t4, 0(t2)
    addi t1, t1, 8
    addi t2, t2, 8
    addi t3, t3, -1
    bnez t3, .Lcopy_user_frame_to_kernel_stack
    j .Lcall_rust

.Ltrap_from_kernel:
    csrr t0, sscratch
    addi sp, sp, -296
    sd t0, 5*8(sp)
    addi t0, sp, 296
    csrw sscratch, x0
    j .Lsave_context_kernel_x5_saved

.Lsave_context_kernel_x5_saved:
    sd x0,  0*8(sp)
    sd x1,  1*8(sp)
    sd t0,  2*8(sp)
    sd x3,  3*8(sp)
    sd x4,  4*8(sp)
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

    csrr t0, sstatus
    sd t0, 32*8(sp)
    csrr t0, sepc
    sd t0, 33*8(sp)
    csrr t0, scause
    sd t0, 34*8(sp)
    csrr t0, stval
    sd t0, 35*8(sp)
    csrr t0, satp
    sd t0, 36*8(sp)
    j .Lcall_rust

.Ltrap_from_kernel_old:
    mv t0, sp
    addi sp, sp, -296

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

    # 保存控制寄存器（GPR 快照完成后再读 sstatus，避免入口临时寄存器污染 x6/x28）
    csrr t0, sstatus
    sd t0, 32*8(sp)
    csrr t0, sepc
    sd t0, 33*8(sp)
    csrr t0, scause
    sd t0, 34*8(sp)
    csrr t0, stval
    sd t0, 35*8(sp)
    csrr t0, satp
    sd t0, 36*8(sp)
    j .Lcall_rust

.Lcall_rust:

    # a0 = cx_ptr
    mv a0, sp
    # 调用 Rust 入口，返回后从当前 sp 上的 TrapContext 恢复现场
    call trap_entry_rust
gdb_point_1:

    # 恢复控制寄存器
    ld t0, 32*8(sp)
    andi t6, t0, 1 << 8
    csrw sstatus, t0
    ld t0, 33*8(sp)
    csrw sepc, t0
    bnez t6, .Ltrap_return_kernel

    mv a0, sp
    addi a1, sp, 296
    j __wateros_riscv_restore_user_from_frame

.Ltrap_return_kernel:

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

    ld t0, 36*8(sp)
    csrw satp, t0
    sfence.vma x0, x0
    ld t0, 2*8(sp)
    mv sp, t0
    sret

# a0 = TrapContext*, a1 = kernel stack top to leave in sscratch after sret.
# This trampoline is mapped in user address spaces. It copies the frame to a
# trampoline data page before switching satp; after the switch it only fetches
# from trampoline text/data and no longer touches kernel stack memory.
__wateros_riscv_restore_user_from_frame:
    la t0, __wateros_riscv_return_frame
    mv t1, a0
    li t2, 37
.Lcopy_return_frame:
    ld t3, 0(t1)
    sd t3, 0(t0)
    addi t1, t1, 8
    addi t0, t0, 8
    addi t2, t2, -1
    bnez t2, .Lcopy_return_frame

    la t0, __wateros_riscv_return_frame
    sd a1, 37*8(t0)

    ld t1, 32*8(t0)
    csrw sstatus, t1
    ld t1, 33*8(t0)
    csrw sepc, t1
    ld t1, 36*8(t0)
    csrw satp, t1
    sfence.vma x0, x0

    csrw sscratch, t0

    ld x1,  1*8(t0)
    ld x3,  3*8(t0)
    ld x4,  4*8(t0)
    ld x6,  6*8(t0)
    ld x7,  7*8(t0)
    ld x8,  8*8(t0)
    ld x9,  9*8(t0)
    ld x10, 10*8(t0)
    ld x11, 11*8(t0)
    ld x12, 12*8(t0)
    ld x13, 13*8(t0)
    ld x14, 14*8(t0)
    ld x15, 15*8(t0)
    ld x16, 16*8(t0)
    ld x17, 17*8(t0)
    ld x18, 18*8(t0)
    ld x19, 19*8(t0)
    ld x20, 20*8(t0)
    ld x21, 21*8(t0)
    ld x22, 22*8(t0)
    ld x23, 23*8(t0)
    ld x24, 24*8(t0)
    ld x25, 25*8(t0)
    ld x26, 26*8(t0)
    ld x27, 27*8(t0)
    ld x28, 28*8(t0)
    ld x29, 29*8(t0)
    ld x30, 30*8(t0)
    ld x31, 31*8(t0)
gdb_point_2:
    ld sp,  2*8(t0)
    ld x5,  5*8(t0)
gdb_point_3:
    sret

    .section .data.trampoline
    .globl __wateros_riscv_kernel_satp, __wateros_riscv_return_frame
    .align 3
__wateros_riscv_kernel_satp:
    .quad 0
__wateros_riscv_return_frame:
    .zero 304
