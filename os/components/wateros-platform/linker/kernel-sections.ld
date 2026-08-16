/* WaterOS 内核公共段布局。平台 wrapper 负责 OUTPUT_ARCH、ENTRY 和装载地址。 */
SECTIONS
{
  . = KERNEL_ENTRY_ADDRESS;
  kernel_start = .;

  text_start = .;
  .text : AT(KERNEL_ENTRY_ADDRESS) {
    KEEP(*(.text.entry.platform))
    KEEP(*(.text.entry.arch))
    . = ALIGN(4K);
    trampoline_start = .;
    *(.text.trampoline)
    *(.text.trampoline.*)
    . = ALIGN(4K);
    *(.text .text.*)
  }
  text_end = .;

  . = ALIGN(4K);
  rodata_start = .;
  .rodata : { *(.rodata .rodata.*) }
  rodata_end = .;

  . = ALIGN(4K);
  data_start = .;
  .data : { *(.data .data.*) }
  data_end = .;

  . = ALIGN(4K);
  bss_start_with_stack = .;
  .bss : {
    *(.bss.stack)
    bss_start = .;
    *(.bss .bss.*)
    *(COMMON)
  }

  . = ALIGN(4K);
  .bss.scheduler : {
    kernel_scheduler_bss_start = .;
    *(.bss.scheduler)
    kernel_scheduler_bss_end = .;
  }
  bss_zero_end = .;

  . = ALIGN(4K);
  .kernel.heap : {
    kernel_heap_start = .;
    *(.kernel.heap)
    kernel_heap_end = .;
  }

  . = ALIGN(4K);
  bss_end = .;
  kernel_end = .;

  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame_hdr)
    *(.note .note.*)
  }
}
