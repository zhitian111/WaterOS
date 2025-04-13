# /Users/code/WaterOS/src/arch/loongarch/trap.asm
# LoongArch 异常与中断处理代码
# 这个文件包含了处理异常和中断的代码

.section .text.trap
.align 2

# 异常向量入口点
.globl trap_vector
trap_vector:
    # 保存上下文到堆栈
    addi.d $sp, $sp, -4*32-5*8  # 分配TrapFrame空间
    
    # 保存通用寄存器
    st.d $zero, $sp, 8*0      # zero寄存器
    st.d $ra, $sp, 8*1        # ra
    st.d $tp, $sp, 8*2        # tp
    # 暂不保存sp，需先保存t0-t1用作临时寄存器
    st.d $a0, $sp, 8*4        # a0
    st.d $a1, $sp, 8*5        # a1
    st.d $a2, $sp, 8*6        # a2
    st.d $a3, $sp, 8*7        # a3
    st.d $a4, $sp, 8*8        # a4
    st.d $a5, $sp, 8*9        # a5
    st.d $a6, $sp, 8*10       # a6
    st.d $a7, $sp, 8*11       # a7
    
    st.d $t0, $sp, 8*12       # t0
    st.d $t1, $sp, 8*13       # t1
    st.d $t2, $sp, 8*14       # t2
    st.d $t3, $sp, 8*15       # t3
    st.d $t4, $sp, 8*16       # t4
    st.d $t5, $sp, 8*17       # t5
    st.d $t6, $sp, 8*18       # t6
    st.d $t7, $sp, 8*19       # t7
    st.d $t8, $sp, 8*20       # t8
    
    # 保存s寄存器
    st.d $s0, $sp, 8*23       # s0
    st.d $s1, $sp, 8*24       # s1
    st.d $s2, $sp, 8*25       # s2
    st.d $s3, $sp, 8*26       # s3
    st.d $s4, $sp, 8*27       # s4
    st.d $s5, $sp, 8*28       # s5
    st.d $s6, $sp, 8*29       # s6
    st.d $s7, $sp, 8*30       # s7
    st.d $s8, $sp, 8*31       # s8
    
    # 获取原始sp值并保存
    csrrd $t0, 0x1            # 读取PRMD
    andi $t1, $t0, 0x3        # 检查PLV位（权限等级）
    beqz $t1, 1f              # 如果是内核态
    # 用户态sp
    csrrd $t0, 0x30           # 读取用户态sp (SAVE0)
    b 2f
1:  # 内核态sp
    addi.d $t0, $sp, 4*32+5*8 # 恢复内核态原始sp
2:  # 保存原始sp
    st.d $t0, $sp, 8*3        # 保存原sp
    
    # 保存CSR寄存器
    csrrd $t0, 0x0            # ERA
    csrrd $t1, 0x1            # PRMD
    st.d $t0, $sp, 4*32       # 保存ERA
    st.d $t1, $sp, 4*32+8     # 保存PRMD
    
    csrrd $t0, 0x7            # BADV
    csrrd $t1, 0x4            # ECFG
    st.d $t0, $sp, 4*32+16    # 保存BADV
    st.d $t1, $sp, 4*32+24    # 保存ECFG
    
    csrrd $t0, 0x5            # ESTAT
    st.d $t0, $sp, 4*32+32    # 保存ESTAT
    
    # 调用Rust的trap处理函数
    move $a0, $sp             # TrapFrame作为参数
    la.pcrel $t0, trap_handler
    jirl $ra, $t0, 0
    
    # 恢复上下文
    ld.d $t0, $sp, 4*32       # 加载ERA
    ld.d $t1, $sp, 4*32+8     # 加载PRMD
    csrwr $t0, 0x0            # 恢复ERA
    csrwr $t1, 0x1            # 恢复PRMD
    
    # 恢复通用寄存器
    ld.d $ra, $sp, 8*1        # ra
    ld.d $tp, $sp, 8*2        # tp
    ld.d $a0, $sp, 8*4        # a0
    ld.d $a1, $sp, 8*5        # a1
    ld.d $a2, $sp, 8*6        # a2
    ld.d $a3, $sp, 8*7        # a3
    ld.d $a4, $sp, 8*8        # a4
    ld.d $a5, $sp, 8*9        # a5
    ld.d $a6, $sp, 8*10       # a6
    ld.d $a7, $sp, 8*11       # a7
    ld.d $t0, $sp, 8*12       # t0
    ld.d $t1, $sp, 8*13       # t1
    ld.d $t2, $sp, 8*14       # t2
    ld.d $t3, $sp, 8*15       # t3
    ld.d $t4, $sp, 8*16       # t4
    ld.d $t5, $sp, 8*17       # t5
    ld.d $t6, $sp, 8*18       # t6
    ld.d $t7, $sp, 8*19       # t7
    ld.d $t8, $sp, 8*20       # t8
    ld.d $s0, $sp, 8*23       # s0
    ld.d $s1, $sp, 8*24       # s1
    ld.d $s2, $sp, 8*25       # s2
    ld.d $s3, $sp, 8*26       # s3
    ld.d $s4, $sp, 8*27       # s4
    ld.d $s5, $sp, 8*28       # s5
    ld.d $s6, $sp, 8*29       # s6
    ld.d $s7, $sp, 8*30       # s7
    ld.d $s8, $sp, 8*31       # s8
    
    # 恢复sp并返回
    ld.d $sp, $sp, 8*3        # 恢复原始sp
    
    # 从异常返回
    ertn
