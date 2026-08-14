OUTPUT_ARCH(riscv)
ENTRY(_start)

/* TODO(real-hardware): verify against the shipping U-Boot environment. */
KERNEL_ENTRY_ADDRESS = 0x40200000;

INCLUDE components/wateros-platform/linker/kernel-sections.ld
