# switch.S — 协作式切换；与 `../src/task.rs` 中 `LoongArch64ArchTaskContext` 布局一致。
# $r4 = 旧上下文指针，$r5 = 新上下文指针；保存 ra($r1)、sp($r3)、$r22–$r31。
    .section .text
    .globl __switch
__switch:
    .cfi_startproc
    st.d $r1,  $r4, 0
    st.d $r3,  $r4, 8
    st.d $r22, $r4, 16
    st.d $r23, $r4, 24
    st.d $r24, $r4, 32
    st.d $r25, $r4, 40
    st.d $r26, $r4, 48
    st.d $r27, $r4, 56
    st.d $r28, $r4, 64
    st.d $r29, $r4, 72
    st.d $r30, $r4, 80
    st.d $r31, $r4, 88

    ld.d $r1,  $r5, 0
    ld.d $r3,  $r5, 8
    ld.d $r22, $r5, 16
    ld.d $r23, $r5, 24
    ld.d $r24, $r5, 32
    ld.d $r25, $r5, 40
    ld.d $r26, $r5, 48
    ld.d $r27, $r5, 56
    ld.d $r28, $r5, 64
    ld.d $r29, $r5, 72
    ld.d $r30, $r5, 80
    ld.d $r31, $r5, 88
    .cfi_undefined 1
    jr $r1
    .cfi_endproc

    .globl __arch_task_entry
__arch_task_entry:
    .cfi_startproc
    move $r4, $r22
    b __wateros_arch_task_entry_trampoline
    .cfi_endproc

    .globl __arch_user_task_entry
__arch_user_task_entry:
    .cfi_startproc
    b __wateros_arch_user_task_entry_trampoline
    .cfi_endproc

    .globl __wateros_arch_restore_user_task
# Rust `TrapContext` 指针在 $r4；$r5 = 用户 trap 时应使用的内核栈顶（与 RISC-V 侧 a1 约定对齐）。
# CSR SAVE0(0x30) 在用户态运行期间保存该内核栈顶，供 trap.S 在 User -> Kernel
# 时先切回内核栈再保存 TrapContext。
__wateros_arch_restore_user_task:
    .cfi_startproc
    move $r21, $r4
    csrwr $r5, 0x30

    # 偏移 32*8 / 33*8：Rust `TrapContext` 的 prmd / era；CSR 0x1=PRMD、0x6=ERA（手册编号）。
    ld.d $r13, $r21, 32*8
    csrwr $r13, 0x1
    ld.d $r13, $r21, 33*8
    csrwr $r13, 0x6
    # 36*8：return_address_space_token，低 48 位 PGDL，高 10 位 ASID。
    ld.d $r13, $r21, 36*8
    bstrpick.d $r14, $r13, 57, 48
    bstrpick.d $r13, $r13, 47, 0
    csrwr $r13, 0x19
    csrwr $r14, 0x18
    csrrd $r13, 0x0
    addi.d $r14, $r0, -9
    and $r13, $r13, $r14
    ori $r13, $r13, 0x10
    csrwr $r13, 0x0

    # TrapContext+37*8 保存 32 个 128-bit LSX 寄存器。
    vld $vr0,  $r21, 296
    vld $vr1,  $r21, 312
    vld $vr2,  $r21, 328
    vld $vr3,  $r21, 344
    vld $vr4,  $r21, 360
    vld $vr5,  $r21, 376
    vld $vr6,  $r21, 392
    vld $vr7,  $r21, 408
    vld $vr8,  $r21, 424
    vld $vr9,  $r21, 440
    vld $vr10, $r21, 456
    vld $vr11, $r21, 472
    vld $vr12, $r21, 488
    vld $vr13, $r21, 504
    vld $vr14, $r21, 520
    vld $vr15, $r21, 536
    vld $vr16, $r21, 552
    vld $vr17, $r21, 568
    vld $vr18, $r21, 584
    vld $vr19, $r21, 600
    vld $vr20, $r21, 616
    vld $vr21, $r21, 632
    vld $vr22, $r21, 648
    vld $vr23, $r21, 664
    vld $vr24, $r21, 680
    vld $vr25, $r21, 696
    vld $vr26, $r21, 712
    vld $vr27, $r21, 728
    vld $vr28, $r21, 744
    vld $vr29, $r21, 760
    vld $vr30, $r21, 776
    vld $vr31, $r21, 792
    ld.d $r13, $r21, 808
    movgr2fcsr $fcsr0, $r13

    ld.d $r1,  $r21, 1*8
    ld.d $r2,  $r21, 2*8
    ld.d $r5,  $r21, 5*8
    ld.d $r6,  $r21, 6*8
    ld.d $r7,  $r21, 7*8
    ld.d $r8,  $r21, 8*8
    ld.d $r9,  $r21, 9*8
    ld.d $r10, $r21, 10*8
    ld.d $r11, $r21, 11*8
    ld.d $r12, $r21, 12*8
    ld.d $r13, $r21, 13*8
    ld.d $r14, $r21, 14*8
    ld.d $r15, $r21, 15*8
    ld.d $r16, $r21, 16*8
    ld.d $r17, $r21, 17*8
    ld.d $r18, $r21, 18*8
    ld.d $r19, $r21, 19*8
    ld.d $r20, $r21, 20*8
    ld.d $r22, $r21, 22*8
    ld.d $r23, $r21, 23*8
    ld.d $r24, $r21, 24*8
    ld.d $r25, $r21, 25*8
    ld.d $r26, $r21, 26*8
    ld.d $r27, $r21, 27*8
    ld.d $r28, $r21, 28*8
    ld.d $r29, $r21, 29*8
    ld.d $r30, $r21, 30*8
    ld.d $r31, $r21, 31*8
    ld.d $r3,  $r21, 3*8
    ld.d $r4,  $r21, 4*8
    ld.d $r21, $r21, 21*8
    ertn
    .cfi_endproc
