  .section .text.entry
  .global _start
  .equ KERNEL_OFFSET, 0xffffffc000000000 // 内核空间的起始地址
_start:
  // 设置boot阶段内核栈空间
  la sp, boot_stack_top
  call kernel_main

  .section .bss.stack
  # 栈底
  .global boot_stack_lower_bound
boot_stack_lower_bound:
  # 栈空间
  .space 4096 * 16
  # 栈顶
  .global boot_stack_top
boot_stack_top:
  
