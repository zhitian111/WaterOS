# 内核入口：`sp` 对齐到 `boot_stack_top` 后 `call kernel_main`（Rust 侧为 `#[no_mangle]`）。
  .section .text.entry
  .global _start
  # 链接脚本/内核虚址布局常量（供将来 PC 相关代码引用；当前启动未使用）。
  .equ KERNEL_OFFSET, 0xffffffc000000000
_start:
  # OpenSBI 进入 S 态后：使用独立 boot 栈，避免破坏固件栈。
  la sp, boot_stack_top
  call kernel_main

  .section .bss.stack
  .global boot_stack_lower_bound
boot_stack_lower_bound:
  .space 4096 * 16
  .global boot_stack_top
boot_stack_top:

