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
    .cfi_startproc
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
    # 新栈属于另一个任务，旧调用者不能再由普通 CFA 表达；切换完成后的真实
    # 调用链从新上下文保存的 ra 继续。
    .cfi_undefined ra
    ret
    .cfi_endproc

    .globl __arch_task_entry
__arch_task_entry:
    .cfi_startproc
    mv a0, s0
    tail __wateros_arch_task_entry_trampoline
    .cfi_endproc

    .globl __arch_user_task_entry
__arch_user_task_entry:
    .cfi_startproc
    tail __wateros_arch_user_task_entry_trampoline
    .cfi_endproc

    .globl __wateros_arch_restore_user_task
# a0 = `TrapContext*`（与 `trap.rs` #[repr(C)] 一致）；a1 = 下次用户态 trap 时使用的内核栈顶。
__wateros_arch_restore_user_task:
    .cfi_startproc
    tail __wateros_riscv_restore_user_from_frame
    .cfi_endproc
