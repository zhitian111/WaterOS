#if defined(__riscv)
    .section .text
    .globl _start
_start:
    la t0, action
    la t1, signal_handler
    sd t1, 0(t0)
    li t1, 4
    sd t1, 8(t0)
    sd zero, 16(t0)

    li a0, 4
    mv a1, t0
    li a2, 0
    li a3, 8
    li a7, 134
    ecall
    bnez a0, failed

    li t0, 1000
    la t1, remaining
    sd t0, 0(t1)
fault:
    .word 0
resume:
    la t1, remaining
    ld t0, 0(t1)
    addi t0, t0, -1
    sd t0, 0(t1)
    bnez t0, fault

    li a0, 1
    la a1, passed_message
    li a2, 73
    li a7, 64
    ecall
    li a0, 0
    li a7, 93
    ecall

signal_handler:
    la t0, resume
    sd t0, 176(a2)
    ret

failed:
    li a0, 1
    la a1, failed_message
    li a2, 46
    li a7, 64
    ecall
    li a0, 1
    li a7, 93
    ecall

#elif defined(__loongarch__)
    .section .text
    .globl _start
_start:
    la.local $t0, action
    la.local $t1, signal_handler
    st.d $t1, $t0, 0
    addi.d $t1, $zero, 4
    st.d $t1, $t0, 8
    st.d $zero, $t0, 16

    addi.d $a0, $zero, 4
    move $a1, $t0
    move $a2, $zero
    addi.d $a3, $zero, 8
    addi.d $a7, $zero, 134
    syscall 0
    bnez $a0, failed

    addi.d $t0, $zero, 1000
    la.local $t1, remaining
    st.d $t0, $t1, 0
fault:
    .word 0
resume:
    la.local $t1, remaining
    ld.d $t0, $t1, 0
    addi.d $t0, $t0, -1
    st.d $t0, $t1, 0
    bnez $t0, fault

    addi.d $a0, $zero, 1
    la.local $a1, passed_message
    addi.d $a2, $zero, 73
    addi.d $a7, $zero, 64
    syscall 0
    move $a0, $zero
    addi.d $a7, $zero, 93
    syscall 0

signal_handler:
    la.local $t0, resume
    st.d $t0, $a2, 176
    jirl $zero, $ra, 0

failed:
    addi.d $a0, $zero, 1
    la.local $a1, failed_message
    addi.d $a2, $zero, 46
    addi.d $a7, $zero, 64
    syscall 0
    addi.d $a0, $zero, 1
    addi.d $a7, $zero, 93
    syscall 0
#else
#error unsupported target
#endif

    .section .rodata
passed_message:
    .ascii "SIGNAL_UCONTEXT_RESULT status=OK iterations=1000 pc_rewrite=OK return=OK\n"
failed_message:
    .ascii "SIGNAL_UCONTEXT_RESULT status=FAIL setup=FAIL\n"

    .section .bss
    .align 4
action:
    .zero 24
remaining:
    .zero 8
