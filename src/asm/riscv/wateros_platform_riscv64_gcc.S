/* 该文件中存储而是 RISC-V 64 架构下的操作系统启动前的准备工作的代码，
  包括设置栈指针、设置页表、调用 Rust 程序的入口函数等。 */
/* 段定义: .text.entry */

  .section .text.entry

/* 符号导出 */
  .globl _start

  .globl boot_stack_lower_bound

  .globl boot_stack_top

  .global sigreturn_trampoline

  .global setup_vm
  
/* 系统宏定义 */

#define STACK_SIZE_LOG2 16
#define STACK_SIZE (1 << STACK_SIZE_LOG2)
#define UART_BASE 0x10000000
#define UART_VIRT_BASE 0x10000000
  .equ KERNEL_OFFSET, 0xffffffc000000000 // 内核空间的起始地址

/*
 * 操作系统的入口函数，这个函数执行以下操作：
 * 1. 设置栈指针
 * 2. 设置页表
 * 3. 调用 Rust 程序的入口函数
*/
_start:
    // 输出提示信息
    li t0, UART_BASE
    li t1, 'B'
    sb t1, 0(t0)
    li t1, 'o'
    sb t1, 0(t0)
    li t1, 'o'
    sb t1, 0(t0)
    li t1, 't'
    sb t1, 0(t0)
    li t1, ' '
    sb t1, 0(t0)
    li t1, 'S'
    sb t1, 0(t0)
    li t1, 't'
    sb t1, 0(t0)
    li t1, 'a'
    sb t1, 0(t0)
    li t1, 'r'
    sb t1, 0(t0)
    li t1, 't'
    sb t1, 0(t0)
    li t1, '!'
    sb t1, 0(t0)
    li t1, '\n'
    sb t1, 0(t0)
    li t1, '\r'
    sb t1, 0(t0)
     
    // 获取核心编号
    // csrr t0, mhartid
    mv t0, a0 // 保存核心编号到 t0 (opensbi启动时传入a0寄存器)
    bnez t0, park_core // 如果是核心 0，则进入休眠状态，等待其他核心启动
    mv tp, t0

    // 设置栈指针, 每个核心分配一个 kstack
    // t0 = hart id
    // pc = 0x00200000
    slli a0, t0, STACK_SIZE_LOG2 // 左移 16 位，得到每个核心的栈大小 (2^16 = 64KB)
    
    // 获取栈顶地址
    la sp, boot_stack_top
    // 设置栈底地址
    sub sp, sp, t0  // sp = stack top - hart_id * stack_size
    // 设置SUM (Supervisor User Memory)位，允许内核和用户模式访问相同的内存
    csrr a0, sstatus // 读取sstatus寄存器
    li a1, 1<<18 // 设置SUM位
    or a0, a0, a1 // 使能SUM位
    csrw sstatus, a0 // 写入sstatus寄存器

    j setup_vm
entry_rust:
    // 跳转到 Rust 程序的入口函数
    // la ra, rust_main  /* 正确加载地址 */
    // 输出提示信息
    li t0, UART_VIRT_BASE
    li t1, 'R'
    sb t1, 0(t0)
    li t1, 'u'
    sb t1, 0(t0)
    li t1, 's'
    sb t1, 0(t0)
    li t1, 't'
    sb t1, 0(t0)
    li t1, 'i'
    sb t1, 0(t0)
    li t1, 'n'
    sb t1, 0(t0)
    li t1, 'g'
    sb t1, 0(t0)
    li t1, '!'
    sb t1, 0(t0)
    li t1, '\n'
    sb t1, 0(t0)
    li t1, '\r'

    // lui ra, %hi(rust_main)
    // jalr ra           /* 跳转 */

    call rust_main  // 调用 Rust 程序的入口函数
    // call rust_main
    // call rust_main


park_core:
  wfi
  j park_core

/*
 * 设置页表
 * 这个函数的作用是设置页表，将内核空间映射到物理地址 0x80000000
*/
setup_vm:

    // since the base addr is 0xffff_ffc0_0020_0000
    // 设置页表寄存器satp
    /*63      60 59                  44 43                                0
     * ---------------------------------------------------------------------
     *|   MODE   |         ASID         |                PPN                |
     * ---------------------------------------------------------------------
    */
    /*RV 64
	 *    ----------------------------------------------------------
	 *   |  Value  |  Name  |  Description                          |
	 *   |----------------------------------------------------------|
	 *   |    0    | Bare   | No translation or protection          |
	 *   |  1 - 7  | ---    | Reserved for standard use             |
	 *   |    8    | Sv39   | Page-based 39 bit virtual addressing  | <-- 我们使用的mode
	 *   |    9    | Sv48   | Page-based 48 bit virtual addressing  |
	 *   |    10   | Sv57   | Page-based 57 bit virtual addressing  |
	 *   |    11   | Sv64   | Page-based 64 bit virtual addressing  |
	 *   | 12 - 13 | ---    | Reserved for standard use             |
	 *   | 14 - 15 | ---    | Reserved for standard use             |
	 *    -----------------------------------------------------------
    */
    la ra, UART_BASE
    li t1, 'B'
    sb t1, 0(ra)
    li t1, 'o'
    sb t1, 0(ra)
    li t1, 'o'
    sb t1, 0(ra)
    li t1, 't'
    sb t1, 0(ra)
    li t1, ' '
    sb t1, 0(ra)
    li t1, 'P'
    sb t1, 0(ra)
    li t1, 'a'
    sb t1, 0(ra)
    li t1, 'g'
    sb t1, 0(ra)
    li t1, 'e'
    sb t1, 0(ra)
    li t1, 't'
    sb t1, 0(ra)
    li t1, 'a'
    sb t1, 0(ra)
    li t1, 'b'
    sb t1, 0(ra)
    li t1, 'l'
    sb t1, 0(ra)
    li t1, 'e'
    sb t1, 0(ra)
    li t1, '!'
    sb t1, 0(ra)
    li t1, '\n'
    sb t1, 0(ra)
    li t1, '\r'
    sb t1, 0(ra)
    la t0, boot_pagetable // 内核启动时的页表物理地址
    li t1, 0x0000008FFFFFFFFF

    and t0, t0, t1 // 虚拟地址转物理地址
    srli t0, t0, 12     // 右移 12 位，得到PNN
    li t1, 8            // 设置MODE段为8, 表示SV39页表格式
    slli t1, t1, 60     // 左移 60 位，得到页表项格式
    or t0, t0, t1       // 合并PNN和页表项格式
    csrw satp, t0       // 写入页表寄存器
    // 刷新TLB
    sfence.vma
    sfence.vma
    sfence.vma
    la ra, entry_rust
    jr ra


/* 
 * 打印寄存器内容
 * 这个函数的作用是打印寄存器a0的内容，用于调试
 * 这个函数遵循标准调用约定，参数a0是需要打印的寄存器，会破坏a0-a4寄存器的值
 * 仅在未开启MMU的时候有效
*/
print_register:
    li a3, 64
print_a_bit:
    li a4, UART_BASE
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

.section .bss.stack
.align 12
boot_stack_lower_bound:
    .space 4096 * 16 * 8  // 8 CPUS at most
/*
 * 位于.data 段的符号，表明系统栈区的栈顶地址
*/
boot_stack_top:




/*
 * 内核启动时使用的页表的起始地址定义
 * 使用SV39页表格式
*/
/* L2 根页表 */
.section .text
.align 12
boot_pagetable:
    /*63      54 53        28 27        19 18        10 9   8 7 6 5 4 3 2 1 0
    * -----------------------------------------------------------------------
    *| Reserved |   PPN[2]   |   PPN[1]   |   PPN[0]   | RSW |D|A|G|U|X|W|R|V|
    * -----------------------------------------------------------------------
    *                                                     |   | | | | | | | |
    *                                                     |   | | | | | | | `---- V - Valid
    *                                                     |   | | | | | | `------ R - Readable
    *                                                     |   | | | | | `-------- W - Writable
    *                                                     |   | | | | `---------- X - Executable
    *                                                     |   | | | `------------ U - User
    *                                                     |   | | `-------------- G - Global
    *                                                     |   | `---------------- A - Accessed
    *                                                     |   `------------------ D - Dirty (0 in page directory)
    *                                                     `---------------------- Reserved for supervisor software
    */
    /* L2 根页表（单级，1GB 巨页） */
    // L2[0]: 1GB 巨页 恒等映射
    .quad (0x00000000 >> 30 << 28) | 0xCF
    // L2[1]: 1GB 巨页 恒等映射
    .quad (0x40000000 >> 30 << 28) | 0xCF 
    // L2[2]: 1GB 巨页 恒等映射
    .quad (0x80000000 >> 30 << 28) | 0xCF 
    // L2[3]: 1GB 巨页 0xC0000000->0x80000000
    .quad (0x80000000 >> 30 << 28) | 0x1F
    // 填充L2[4]到L2[98] 
    .zero 8 * 254
    // L2[258]: 1GB 巨页 映射到内核空间
    .quad (0x80000000 >> 30 << 28) | 0xCF
    // 填充剩余表项
    .zero 8

.section .text.trampoline
.align 12
/*
 * 信号处理函数的中断陷阱
 * 这个函数的作用是将控制权转移到 Rust 程序的信号处理函数中
 * 也可以用来测试ecall指令的功能
*/
sigreturn_trampoline:
    li	a7,139
    ecall
