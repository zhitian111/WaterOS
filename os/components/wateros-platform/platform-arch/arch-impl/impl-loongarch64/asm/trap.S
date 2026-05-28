    .section .text.trampoline
    .globl __alltraps
    .align 2

# Minimal LoongArch64 trap frame snapshot.
# Layout matches rust::TrapContext:
# - x[0..31] : 32 * 8
# - prmd     : 32 * 8
# - era      : 33 * 8
# - estat    : 34 * 8
# - badv     : 35 * 8
# - return_address_space_token : 36 * 8
#
# CSR SAVE0 stores the current task's kernel stack top while running in user mode.
# User -> kernel traps swap $r3 with SAVE0 before building TrapContext, mirroring the
# RISC-V sscratch convention used by the sibling implementation.
    .equ TRAP_CONTEXT_SIZE, 296
    .equ CSR_PRMD, 0x1
    .equ CSR_ERA, 0x6
    .equ CSR_ESTAT, 0x5
    .equ CSR_BADV, 0x7
    .equ CSR_CRMD, 0x0
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

__alltraps:
    # Stash temporaries before using them to decide whether we must switch stacks.
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
    # SAVE0 holds the current task's kernel stack top while PLV3 is running.
    # Preserve the user stack in r15 for x[3], then build TrapContext on the
    # kernel stack.
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
    st.d $r0,  $r3, 288

    move $r4, $r3
    bl trap_entry_rust

    ld.d $r12, $r3, 256
    andi $r13, $r12, PRMD_PPLV_MASK
    addi.d $r14, $r0, USER_PLV
    csrwr $r12, CSR_PRMD
    ld.d $r12, $r3, 264
    csrwr $r12, CSR_ERA

    beq $r13, $r14, .Lreturn_to_user
    RESTORE_GENERAL_REGS
    addi.d $r3, $r3, TRAP_CONTEXT_SIZE
    ertn

.Lreturn_to_user:
    # TrapContext.return_address_space_token is the PGDL base address to use in user mode.
    ld.d $r12, $r3, 36*8
    csrwr $r12, CSR_PGDL
    invtlb 0, $r0, $r0
    csrrd $r12, CSR_CRMD
    addi.d $r13, $r0, -9
    and $r12, $r12, $r13
    ori $r12, $r12, CRMD_PG
    csrwr $r12, CSR_CRMD
    addi.d $r12, $r3, TRAP_CONTEXT_SIZE
    csrwr $r12, CSR_SAVE0
    RESTORE_GENERAL_REGS
    ld.d $r3, $r3, 3*8
    ertn

    .section .text.tlb_refill
    .globl __tlb_refill
    .align 12
__tlb_refill:
    csrwr $r12, CSR_TLBRSAVE
    csrrd $r12, CSR_PGD
    lddir $r12, $r12, 2
    lddir $r12, $r12, 1
    ldpte $r12, 0
    ldpte $r12, 1
    tlbfill
    csrrd $r12, CSR_TLBRSAVE
    ertn
