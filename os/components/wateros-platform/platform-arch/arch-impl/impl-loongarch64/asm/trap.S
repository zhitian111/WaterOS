    .section .text.trampoline
    .globl __alltraps
    .align 2

# LoongArch64 最小 trap 帧快照。
# 布局与 rust::TrapContext 一致：
# - x[0..31] : 32 * 8
# - prmd     : 32 * 8
# - era      : 33 * 8
# - estat    : 34 * 8
# - badv     : 35 * 8
# - return_address_space_token : 36 * 8
# - lsx      : 37 * 8, 32 * 16 bytes
# - fcsr     : 101 * 8
#
# CSR SAVE0 在用户态运行时保存当前任务的内核栈顶。
# 用户态进入内核 trap 时，在构建 TrapContext 前用 SAVE0 与 $r3 交换，与 RISC-V
# 实现中的 sscratch 约定对应。
    .equ TRAP_CONTEXT_SIZE, 816
    .equ CSR_PRMD, 0x1
    .equ CSR_ERA, 0x6
    .equ CSR_ESTAT, 0x5
    .equ CSR_BADV, 0x7
    .equ CSR_CRMD, 0x0
    .equ CSR_ASID, 0x18
    .equ CSR_PGDL, 0x19
    .equ CSR_PGD, 0x1B
    .equ CSR_SAVE0, 0x30
    .equ CSR_SAVE1, 0x31
    .equ CSR_SAVE2, 0x32
    .equ CSR_SAVE3, 0x33
    .equ CSR_SAVE4, 0x34
    .equ CSR_TLBRSAVE, 0x8B
    .equ PRMD_PPLV_MASK, 0x3
    .equ USER_PLV, 0x3
    .equ CRMD_PG, 0x10

    .macro RESTORE_GENERAL_REGS
    ld.d $r1,  $r3, 8
    ld.d $r2,  $r3, 16
    ld.d $r4,  $r3, 32
    ld.d $r5,  $r3, 40
    ld.d $r6,  $r3, 48
    ld.d $r7,  $r3, 56
    ld.d $r8,  $r3, 64
    ld.d $r9,  $r3, 72
    ld.d $r10, $r3, 80
    ld.d $r11, $r3, 88
    ld.d $r12, $r3, 96
    ld.d $r13, $r3, 104
    ld.d $r14, $r3, 112
    ld.d $r15, $r3, 120
    ld.d $r16, $r3, 128
    ld.d $r17, $r3, 136
    ld.d $r18, $r3, 144
    ld.d $r19, $r3, 152
    ld.d $r20, $r3, 160
    ld.d $r21, $r3, 168
    ld.d $r22, $r3, 176
    ld.d $r23, $r3, 184
    ld.d $r24, $r3, 192
    ld.d $r25, $r3, 200
    ld.d $r26, $r3, 208
    ld.d $r27, $r3, 216
    ld.d $r28, $r3, 224
    ld.d $r29, $r3, 232
    ld.d $r30, $r3, 240
    ld.d $r31, $r3, 248
    .endm

    .macro SAVE_LSX_REGS
    vst $vr0,  $r3, 296
    vst $vr1,  $r3, 312
    vst $vr2,  $r3, 328
    vst $vr3,  $r3, 344
    vst $vr4,  $r3, 360
    vst $vr5,  $r3, 376
    vst $vr6,  $r3, 392
    vst $vr7,  $r3, 408
    vst $vr8,  $r3, 424
    vst $vr9,  $r3, 440
    vst $vr10, $r3, 456
    vst $vr11, $r3, 472
    vst $vr12, $r3, 488
    vst $vr13, $r3, 504
    vst $vr14, $r3, 520
    vst $vr15, $r3, 536
    vst $vr16, $r3, 552
    vst $vr17, $r3, 568
    vst $vr18, $r3, 584
    vst $vr19, $r3, 600
    vst $vr20, $r3, 616
    vst $vr21, $r3, 632
    vst $vr22, $r3, 648
    vst $vr23, $r3, 664
    vst $vr24, $r3, 680
    vst $vr25, $r3, 696
    vst $vr26, $r3, 712
    vst $vr27, $r3, 728
    vst $vr28, $r3, 744
    vst $vr29, $r3, 760
    vst $vr30, $r3, 776
    vst $vr31, $r3, 792
    .endm

    .macro RESTORE_LSX_REGS base
    vld $vr0,  \base, 296
    vld $vr1,  \base, 312
    vld $vr2,  \base, 328
    vld $vr3,  \base, 344
    vld $vr4,  \base, 360
    vld $vr5,  \base, 376
    vld $vr6,  \base, 392
    vld $vr7,  \base, 408
    vld $vr8,  \base, 424
    vld $vr9,  \base, 440
    vld $vr10, \base, 456
    vld $vr11, \base, 472
    vld $vr12, \base, 488
    vld $vr13, \base, 504
    vld $vr14, \base, 520
    vld $vr15, \base, 536
    vld $vr16, \base, 552
    vld $vr17, \base, 568
    vld $vr18, \base, 584
    vld $vr19, \base, 600
    vld $vr20, \base, 616
    vld $vr21, \base, 632
    vld $vr22, \base, 648
    vld $vr23, \base, 664
    vld $vr24, \base, 680
    vld $vr25, \base, 696
    vld $vr26, \base, 712
    vld $vr27, \base, 728
    vld $vr28, \base, 744
    vld $vr29, \base, 760
    vld $vr30, \base, 776
    vld $vr31, \base, 792
    .endm

__alltraps:
    .cfi_startproc
    .cfi_signal_frame
    # 先用临时寄存器保存现场，再判断是否需切换栈。
    csrwr $r12, CSR_SAVE1
    csrwr $r13, CSR_SAVE2
    csrwr $r14, CSR_SAVE3
    csrwr $r15, CSR_SAVE4

    csrrd $r12, CSR_PRMD
    andi $r13, $r12, PRMD_PPLV_MASK
    addi.d $r14, $r0, USER_PLV
    beq $r13, $r14, .Ltrap_from_user

.Ltrap_from_kernel:
    move $r15, $r3
    addi.d $r3, $r3, -TRAP_CONTEXT_SIZE
    b .Lsave_context

.Ltrap_from_user:
    # PLV3 运行时 SAVE0 保存当前任务内核栈顶；用户栈暂存于 r15（对应 x[3]），
    # 再在内核栈上构建 TrapContext。
    move $r15, $r3
    csrrd $r3, CSR_SAVE0
    addi.d $r3, $r3, -TRAP_CONTEXT_SIZE

.Lsave_context:

    st.d $r0,  $r3, 0
    st.d $r1,  $r3, 8
    st.d $r2,  $r3, 16
    st.d $r15, $r3, 24
    st.d $r4,  $r3, 32
    st.d $r5,  $r3, 40
    st.d $r6,  $r3, 48
    st.d $r7,  $r3, 56
    st.d $r8,  $r3, 64
    st.d $r9,  $r3, 72
    st.d $r10, $r3, 80
    st.d $r11, $r3, 88
    csrrd $r12, CSR_SAVE1
    st.d $r12, $r3, 96
    csrrd $r12, CSR_SAVE2
    st.d $r12, $r3, 104
    csrrd $r12, CSR_SAVE3
    st.d $r12, $r3, 112
    csrrd $r12, CSR_SAVE4
    st.d $r12, $r3, 120
    st.d $r16, $r3, 128
    st.d $r17, $r3, 136
    st.d $r18, $r3, 144
    st.d $r19, $r3, 152
    st.d $r20, $r3, 160
    st.d $r21, $r3, 168
    st.d $r22, $r3, 176
    st.d $r23, $r3, 184
    st.d $r24, $r3, 192
    st.d $r25, $r3, 200
    st.d $r26, $r3, 208
    st.d $r27, $r3, 216
    st.d $r28, $r3, 224
    st.d $r29, $r3, 232
    st.d $r30, $r3, 240
    st.d $r31, $r3, 248

    # CSR 编号与 `trap.rs` 中 `TrapContext` 字段：0x1=PRMD、0x6=ERA、0x5=ESTAT、0x7=BADV。
    csrrd $r12, CSR_PRMD
    st.d $r12, $r3, 256
    csrrd $r12, CSR_ERA
    st.d $r12, $r3, 264
    csrrd $r12, CSR_ESTAT
    st.d $r12, $r3, 272
    csrrd $r12, CSR_BADV
    st.d $r12, $r3, 280
    # 保存 trap 入口时激活的地址空间 token：低 48 位为 PGDL，高 10 位为 ASID。
    csrrd $r12, CSR_PGDL
    csrrd $r13, CSR_ASID
    bstrins.d $r12, $r13, 57, 48
    st.d $r12, $r3, 288
    SAVE_LSX_REGS
    movfcsr2gr $r12, $fcsr0
    st.d $r12, $r3, 808

    move $r4, $r3
    # r1/ra 位于 TrapContext+8，CFA 为进入 trap 前的内核栈顶。
    .cfi_def_cfa 3, TRAP_CONTEXT_SIZE
    .cfi_offset 1, -288
    bl trap_entry_rust

    ld.d $r12, $r3, 256
    andi $r13, $r12, PRMD_PPLV_MASK
    addi.d $r14, $r0, USER_PLV
    csrwr $r12, CSR_PRMD
    ld.d $r12, $r3, 264
    csrwr $r12, CSR_ERA

    beq $r13, $r14, .Lreturn_to_user
    RESTORE_LSX_REGS $r3
    ld.d $r12, $r3, 808
    movgr2fcsr $fcsr0, $r12
    RESTORE_GENERAL_REGS
    addi.d $r3, $r3, TRAP_CONTEXT_SIZE
    ertn

.Lreturn_to_user:
    # TrapContext token：低 48 位为 PGDL，bit[57:48] 为 ASID。
    # ASID 分配/复用时的 shootdown 由 MM 保证，因此地址空间切换只需原子式
    # 更新 PGDL/ASID，不再清空本 CPU 的其它地址空间 TLB。
    ld.d $r12, $r3, 36*8
    bstrpick.d $r14, $r12, 57, 48
    bstrpick.d $r12, $r12, 47, 0
    csrrd $r13, CSR_PGDL
    bne $r12, $r13, .Lswitch_user_aspace
    csrrd $r15, CSR_ASID
    andi $r15, $r15, 0x3ff
    beq $r14, $r15, .Luser_aspace_ready
.Lswitch_user_aspace:
    csrwr $r12, CSR_PGDL
    csrwr $r14, CSR_ASID
.Luser_aspace_ready:
    csrrd $r12, CSR_CRMD
    addi.d $r13, $r0, -9
    and $r12, $r12, $r13
    ori $r12, $r12, CRMD_PG
    csrwr $r12, CSR_CRMD
    addi.d $r12, $r3, TRAP_CONTEXT_SIZE
    csrwr $r12, CSR_SAVE0
    RESTORE_LSX_REGS $r3
    ld.d $r12, $r3, 808
    movgr2fcsr $fcsr0, $r12
    RESTORE_GENERAL_REGS
    ld.d $r3, $r3, 3*8
    ertn
    .cfi_endproc

    .section .text.tlb_refill
    .globl __tlb_refill
    .align 12
__tlb_refill:
    csrwr $r12, CSR_TLBRSAVE
    csrrd $r12, CSR_PGD
    lddir $r12, $r12, 3
    lddir $r12, $r12, 1
    ldpte $r12, 0
    ldpte $r12, 1
    tlbfill
    csrrd $r12, CSR_TLBRSAVE
    ertn
