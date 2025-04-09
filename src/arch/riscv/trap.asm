.global trap_handler
trap_handler:
    # 保存上下文
    csrrw sp, sscratch, sp
    # ... 保存寄存器到栈

    # 检查陷阱原因
    csrr a0, scause
    csrr a1, sepc

    # 处理系统调用/断点
    li t0, 0x8
    beq a0, t0, handle_ebreak

    # ... 其他处理逻辑

handle_ebreak:
    # 处理ebreak（示例：打印信息后返回）
    addi a1, a1, 4  # 跳过ebreak指令
    csrw sepc, a1
    # ... 恢复寄存器
    sret
