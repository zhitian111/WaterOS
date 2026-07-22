# QEMU virt + OpenSBI 入口 shim。
# OpenSBI: a0 = hart id, a1 = DTB physical address.
# arch boot ABI: a0 = logical CPU id, a1/a2 = platform arguments.
  .section .text.entry.platform
  .globl _start
_start:
  li a2, 0
  tail __wateros_arch_boot
