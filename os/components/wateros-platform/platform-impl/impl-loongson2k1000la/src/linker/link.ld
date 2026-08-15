OUTPUT_ARCH(loongarch)
ENTRY(_start)

KERNEL_ENTRY_ADDRESS = 0x98000000;

INCLUDE components/wateros-platform/linker/kernel-sections.ld
