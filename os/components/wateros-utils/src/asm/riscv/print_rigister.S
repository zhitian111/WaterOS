/* 
 * 打印寄存器内容
 * 这个函数的作用是打印寄存器a0的内容，用于调试
 * 这个函数遵循标准调用约定，参数a0是需要打印的寄存器，会破坏a0-a4寄存器的值
 * 仅在未开启MMU的时候有效
*/
print_register:
    li a3, 64
print_a_bit:
    li a4, 0x10000000
    li a1, 0x8000000000000000
    and a2, a0, a1
    srli a2, a2, 63
    add a2, a2, '0'
    sb a2, 0(a4)
    slli a0, a0, 1
    addi a3, a3, -1
    bnez a3, print_a_bit
    li a2, '\n'
    sb a2, 0(a4)
    ret
