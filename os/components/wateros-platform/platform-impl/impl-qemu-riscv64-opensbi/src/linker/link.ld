OUTPUT_ARCH(riscv)
ENTRY(_start)

QEMU_ENTRY_ADDRESS = 0x80200000; /* 8020_0000 */

KERNEL_OFFSET = 0xffffffc000000000; /* 内核空间的起始地址*/
RAM_START    = 0x0000000080000000;
SECTIONS{
  . = QEMU_ENTRY_ADDRESS;
  kernel_start = .;
  /* text 段 */
  text_start = .;
  .text : AT(QEMU_ENTRY_ADDRESS){
    *(.text.entry)
    . = ALIGN(4K);
    trampoline_start = .;
    *(.text.trampoline)
    *(.text.trampoline.*)

    . = ALIGN(4K);
    *(.text.*)
  }
  text_end = .;
  /* rodata 段 */
  . = ALIGN(4K);

  rodata_start = .;
  .rodata : {
    *(.rodata)
    *(.rodata.*)
    *(.rodata*)
  }
  rodata_end = .;

  . = ALIGN(4K);
  data_start = .;
  .data : {
    *(.data .data.*)
  }
  data_end = .;
  /* bss 段 */
  . = ALIGN(4K);
  bss_start_with_stack = .;
  .bss : {
    /* 栈段 */
    *(.bss.stack)
    bss_start = .;
    *(.bss .bss.*)
  }
  . = ALIGN(4K);
  /* 调度器等关键全局状态：必须在 128MiB 堆池之前，避免堆元数据覆盖 */
  .bss.scheduler : {
    kernel_scheduler_bss_start = .;
    *(.bss.scheduler)
    kernel_scheduler_bss_end = .;
  }
  . = ALIGN(4K);
  /* 128 MiB 堆池置于 BSS 末尾：越界时先进入帧池区，而非覆盖调度器等全局状态 */
  .kernel.heap : {
    kernel_heap_start = .;
    *(.kernel.heap)
    kernel_heap_end = .;
  }
  . = ALIGN(4K);
  bss_end = .;
  . = ALIGN(4K);
  kernel_end = .;
}
