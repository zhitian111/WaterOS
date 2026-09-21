# VisionFive 2 OpenSBI entry. a0=hart id, a1=DTB physical address.
  .section .text.entry.platform
  .globl _start
_start:
  li a2, 0
  tail __wateros_arch_boot
