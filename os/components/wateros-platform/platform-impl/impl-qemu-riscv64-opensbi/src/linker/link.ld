OUTPUT_ARCH(riscv)
ENTRY(_start)

KERNEL_ENTRY_ADDRESS = 0x80200000;

INCLUDE components/wateros-platform/linker/kernel-sections.ld
