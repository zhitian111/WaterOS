# /Users/code/WaterOS/src/arch/loongarch/entry.asm
# LoongArch启动入口点
# 这个文件包含了系统启动入口和上下文切换代码

.section .text.entry
.globl _start
_start:
    # 设置堆栈指针
    la.pcrel $sp, boot_stack_top

    # 清空bss段
    la.pcrel $t0, sbss
    la.pcrel $t1, ebss
    beq $t0, $t1, skip_bss_clear

clear_bss_loop:
    st.w $zero, $t0, 0
    addi.d $t0, $t0, 4
    blt $t0, $t1, clear_bss_loop

skip_bss_clear:
    # 调用Rust入口点
    la.pcrel $t0, rust_main
    jirl $ra, $t0, 0

    # 不应该返回到这里，但如果返回了，则进入无限循环
halt_loop:
    j halt_loop

# 用户上下文恢复函数
.globl restore_user_context
restore_user_context:
    # a0寄存器中保存了TrapFrame结构体的指针

    # 加载CSR寄存器
    ld.d $t0, $a0, 4*32     # 加载csr_era（返回地址）
    ld.d $t1, $a0, 4*32+8   # 加载csr_prmd（运行模式）
    csrwr $t0, 0x0          # 设置CSR_ERA（异常返回地址）
    csrwr $t1, 0x1          # 设置CSR_PRMD（先前运行模式）

    # 加载通用寄存器
    ld.d $ra, $a0, 8*1      # ra
    ld.d $tp, $a0, 8*2      # tp
    ld.d $sp, $a0, 8*3      # sp
    ld.d $a1, $a0, 8*5      # a1
    ld.d $a2, $a0, 8*6      # a2
    ld.d $a3, $a0, 8*7      # a3
    ld.d $a4, $a0, 8*8      # a4
    ld.d $a5, $a0, 8*9      # a5
    ld.d $a6, $a0, 8*10     # a6
    ld.d $a7, $a0, 8*11     # a7
    ld.d $t0, $a0, 8*12     # t0
    ld.d $t1, $a0, 8*13     # t1
    ld.d $t2, $a0, 8*14     # t2
    ld.d $t3, $a0, 8*15     # t3
    ld.d $t4, $a0, 8*16     # t4
    ld.d $t5, $a0, 8*17     # t5
    ld.d $t6, $a0, 8*18     # t6
    ld.d $t7, $a0, 8*19     # t7
    ld.d $t8, $a0, 8*20     # t8
    ld.d $s0, $a0, 8*23     # s0
    ld.d $s1, $a0, 8*24     # s1
    ld.d $s2, $a0, 8*25     # s2
    ld.d $s3, $a0, 8*26     # s3
    ld.d $s4, $a0, 8*27     # s4
    ld.d $s5, $a0, 8*28     # s5
    ld.d $s6, $a0, 8*29     # s6
    ld.d $s7, $a0, 8*30     # s7
    ld.d $s8, $a0, 8*31     # s8
    ld.d $a0, $a0, 8*4      # a0 最后加载，避免覆盖

    # 从异常返回到用户态
    ertn

.section .bss.stack
.align 12  # 4KB对齐
.global boot_stack
boot_stack:
    .space 4096 * 4  # 16KB内核栈
.global boot_stack_top
boot_stack_top:
