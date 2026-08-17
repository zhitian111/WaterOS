# trap.asm — WaterOS RISC-V S-mode 异常/中断入口（方案A）。
# 与 `../src/trap.rs` 中 `TrapContext`、`trap_entry_rust` 成对维护；布局变更须同步 Rust `#[repr(C)]`。
    .section .text.trampoline
    .option arch, +d
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
# - f[0..31] : 37 * 8
# - fcsr     : 69 * 8（低 32 位；尾部 4 字节为对齐填充）
# 总大小：70 * 8 = 560 字节
#
# a0 = TrapContext*  (交给 trap_entry_rust)
#
# 约定：
# - 从用户态进入 trap 前，sscratch 指向 supervisor-only trampoline frame。
# - trampoline frame 的 37*8 槽保存当前任务内核栈顶，供入口切到内核栈。
# - 返回用户态时，sscratch 重新指向 trampoline frame，用户 sp 从 TrapContext 恢复。

__alltraps:
    .cfi_startproc
    .cfi_signal_frame
    # 用户态进入时 sscratch = __wateros_riscv_return_frame。
    # 内核态保持 sscratch = 0。首次 csrrw 把用户 x5/t0 存入 sscratch，同时拿到
    # 跳板帧指针，快照前不必先破坏其它用户 GPR。
    csrrw t0, sscratch, t0
    beqz t0, .Ltrap_from_kernel
gdb_point_0:
    # 仍在用户页表下，把完整用户 GPR 集写入跳板帧。帧页仅监管态可访问但 S 态可写，
    # 切回内核 satp 前只需这一页可写数据。
    sd x0,  0*8(t0)
    sd x1,  1*8(t0)
    sd sp,  2*8(t0)
    sd x3,  3*8(t0)
    sd x4,  4*8(t0)
    sd x6,  6*8(t0)
    csrr t1, sscratch
    sd t1, 5*8(t0)
    # x5/t0 已保存后，清零 sscratch 再继续访存；否则本路径若嵌套 S 态 fault，
    # 会带着用户 t0 重入并误当作帧指针。
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
    addi sp, sp, -560
    la t1, __wateros_riscv_kernel_satp
    ld t1, 0(t1)
    la t2, __wateros_riscv_asid_enabled
    ld t2, 0(t2)
    csrw satp, t1
    bnez t2, .Lkernel_aspace_ready
    sfence.vma x0, x0
.Lkernel_aspace_ready:

    # t0 是 sscratch 给出的本 CPU return frame。用户 tp 是 TLS，必须在进入
    # 任何 Rust 代码前从 supervisor-only 槽恢复可信 CPU id。
    ld tp, 38*8(t0)

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
    addi sp, sp, -560
    sd t0, 5*8(sp)
    addi t0, sp, 560
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
    addi sp, sp, -560

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

    # 用户态可在任意 F/D 指令后被时钟抢占。必须在调用 Rust（以及可能的
    # 调度切换）前把完整 FPU 状态放进当前任务内核栈上的 TrapContext。
    fsd f0,  296(sp)
    fsd f1,  304(sp)
    fsd f2,  312(sp)
    fsd f3,  320(sp)
    fsd f4,  328(sp)
    fsd f5,  336(sp)
    fsd f6,  344(sp)
    fsd f7,  352(sp)
    fsd f8,  360(sp)
    fsd f9,  368(sp)
    fsd f10, 376(sp)
    fsd f11, 384(sp)
    fsd f12, 392(sp)
    fsd f13, 400(sp)
    fsd f14, 408(sp)
    fsd f15, 416(sp)
    fsd f16, 424(sp)
    fsd f17, 432(sp)
    fsd f18, 440(sp)
    fsd f19, 448(sp)
    fsd f20, 456(sp)
    fsd f21, 464(sp)
    fsd f22, 472(sp)
    fsd f23, 480(sp)
    fsd f24, 488(sp)
    fsd f25, 496(sp)
    fsd f26, 504(sp)
    fsd f27, 512(sp)
    fsd f28, 520(sp)
    fsd f29, 528(sp)
    fsd f30, 536(sp)
    fsd f31, 544(sp)
    csrr t0, fcsr
    sw t0, 552(sp)

    # TrapContext 位于 CFA 下方：x1/ra 在 sp+8，原始 sp 在 sp+16。
    # 这使 GDB 能从 Rust trap handler 回到被中断的内核调用链。
    .cfi_def_cfa sp, 560
    .cfi_offset ra, -552

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
    addi a1, sp, 560
    j __wateros_riscv_restore_user_from_frame

.Ltrap_return_kernel:

    # 内核返回直接使用当前栈帧恢复 FPU。用户返回由
    # __wateros_riscv_restore_user_from_frame 在切换 satp 前恢复；不能在公共
    # 路径先恢复一次，否则每个用户 trap 会重复执行 32 次 fld 和一次 fcsr 写入。
    fld f0,  296(sp)
    fld f1,  304(sp)
    fld f2,  312(sp)
    fld f3,  320(sp)
    fld f4,  328(sp)
    fld f5,  336(sp)
    fld f6,  344(sp)
    fld f7,  352(sp)
    fld f8,  360(sp)
    fld f9,  368(sp)
    fld f10, 376(sp)
    fld f11, 384(sp)
    fld f12, 392(sp)
    fld f13, 400(sp)
    fld f14, 408(sp)
    fld f15, 416(sp)
    fld f16, 424(sp)
    fld f17, 432(sp)
    fld f18, 440(sp)
    fld f19, 448(sp)
    fld f20, 456(sp)
    fld f21, 464(sp)
    fld f22, 472(sp)
    fld f23, 480(sp)
    fld f24, 488(sp)
    fld f25, 496(sp)
    fld f26, 504(sp)
    fld f27, 512(sp)
    fld f28, 520(sp)
    fld f29, 528(sp)
    fld f30, 536(sp)
    fld f31, 544(sp)
    lw t0, 552(sp)
    csrw fcsr, t0

    ld x1,  1*8(sp)
    ld x3,  3*8(sp)
    ld x4,  4*8(sp)
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
    la t1, __wateros_riscv_asid_enabled
    ld t1, 0(t1)
    csrw satp, t0
    bnez t1, .Lold_user_aspace_ready
    sfence.vma x0, x0
.Lold_user_aspace_ready:
    # t0/t1 were scratch registers for satp/ASID above. Restore them only after
    # all return preparation is complete; restoring them earlier corrupts every
    # interrupted kernel task (ASID enabled made t1 equal to 1).
    ld x5,  5*8(sp)
    ld x6,  6*8(sp)
    ld sp,  2*8(sp)
    sret

    .cfi_endproc

# a0 = TrapContext*，a1 = sret 后写入 sscratch 的内核栈顶。
# 该跳板映射在用户地址空间内：切 satp 前把帧复制到跳板数据页；切换后只从跳板
# 代码/数据取指，不再访问内核栈内存。
__wateros_riscv_restore_user_from_frame:
    # 首次进入用户态以及 exec/fork 恢复也必须装载 TrapContext 中的 FPU 状态。
    # 这里仍在内核页表下，之后切换 satp 的 trampoline 不再访问浮点状态。
    fld f0,  296(a0)
    fld f1,  304(a0)
    fld f2,  312(a0)
    fld f3,  320(a0)
    fld f4,  328(a0)
    fld f5,  336(a0)
    fld f6,  344(a0)
    fld f7,  352(a0)
    fld f8,  360(a0)
    fld f9,  368(a0)
    fld f10, 376(a0)
    fld f11, 384(a0)
    fld f12, 392(a0)
    fld f13, 400(a0)
    fld f14, 408(a0)
    fld f15, 416(a0)
    fld f16, 424(a0)
    fld f17, 432(a0)
    fld f18, 440(a0)
    fld f19, 448(a0)
    fld f20, 456(a0)
    fld f21, 464(a0)
    fld f22, 472(a0)
    fld f23, 480(a0)
    fld f24, 488(a0)
    fld f25, 496(a0)
    fld f26, 504(a0)
    fld f27, 512(a0)
    fld f28, 520(a0)
    fld f29, 528(a0)
    fld f30, 536(a0)
    fld f31, 544(a0)
    lw t1, 552(a0)
    csrw fcsr, t1

    # 内核 tp 是可信 CPU id；选择本 CPU 独占的 320 字节 return frame。
    # 320 = 256 + 64，避免 trampoline 依赖乘法扩展。
    slli t4, tp, 8
    slli t3, tp, 6
    add t4, t4, t3
    la t5, __wateros_riscv_return_frames
    add t5, t5, t4
    mv t0, t5
    mv t1, a0
    li t2, 37
.Lcopy_return_frame:
    ld t3, 0(t1)
    sd t3, 0(t0)
    addi t1, t1, 8
    addi t0, t0, 8
    addi t2, t2, -1
    bnez t2, .Lcopy_return_frame

    mv t0, t5
    sd a1, 37*8(t0)
    sd tp, 38*8(t0)            # 保存内核 tp（hart_id），下次用户 trap 时恢复

    ld t1, 32*8(t0)
    csrw sstatus, t1
    ld t1, 33*8(t0)
    csrw sepc, t1
    la t2, __wateros_riscv_asid_enabled
    ld t2, 0(t2)
    ld t1, 36*8(t0)
    csrw satp, t1
    bnez t2, .Luser_aspace_ready
    sfence.vma x0, x0
.Luser_aspace_ready:

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
    .globl __wateros_riscv_kernel_satp, __wateros_riscv_asid_enabled
    .globl __wateros_riscv_return_frame, __wateros_riscv_return_frames
    .balign 4096
__wateros_riscv_kernel_satp:
    .quad 0
__wateros_riscv_asid_enabled:
    .quad 0
    .balign 4096
__wateros_riscv_return_frames:
__wateros_riscv_return_frame:
    # 每 CPU 40 槽：0-36 用户 GPR/CSR，37=kernel_stack_top，38=kernel_cpu_id。
    # FPU 状态在切换用户 satp 前直接从内核 TrapContext 恢复，不必复制到这里。
    # 8 * 320 = 2560 字节，完整位于同一个 trampoline 数据页。
    .zero 320 * 8
