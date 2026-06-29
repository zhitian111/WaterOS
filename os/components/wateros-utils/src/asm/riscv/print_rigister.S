/*
 * 调试辅助：将 a0 以 64 位二进制形式输出到 UART（0x10000000）。
 *
 * 遵循标准调用约定，参数为待打印的寄存器值；会破坏 a0–a4。
 * 仅在未开启 MMU 的早期 bring-up 阶段使用；尚未接入 `lib.rs` 构建。
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
