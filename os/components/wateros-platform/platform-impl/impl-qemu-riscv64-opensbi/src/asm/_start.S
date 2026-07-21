# 内核入口：所有 hart 从此进入，根据 hart_id 选择 per-CPU 栈后 call kernel_main。
  .section .text.entry
  .global _start
  .equ KERNEL_OFFSET, 0xffffffc000000000
  .equ STACK_PER_CPU, 4096 * 16
_start:
  # a0 = hart_id (OpenSBI 传入)
  mv   tp, a0                 # 保存 hart_id 到 tp（供 current_cpu_id 读取）
  slli t0, a0, 14            # t0 = hart_id * 16384 (2^14)
  la   t1, cpu_stacks
  add  t1, t1, t0             # t1 = cpu_stacks + offset = 本 CPU 栈区基址
  add  sp, t1, x0             # sp = 栈区基址
  li   t0, STACK_PER_CPU
  add  sp, sp, t0             # sp += 16384 → 栈顶
  call kernel_main

  .section .bss.stack
  .global cpu_stacks
  .balign 4096
cpu_stacks:
  .space 4096 * 16 * 8      # 8 CPUs × 16KB = 128KB

