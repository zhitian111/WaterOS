OUTPUT_ARCH(loongarch)
ENTRY(_start)

QEMU_ENTRY_ADDRESS = 0x90000000;

SECTIONS
{
  . = QEMU_ENTRY_ADDRESS;
  kernel_start = .;

  text_start = .;
  .text : AT(QEMU_ENTRY_ADDRESS) {
    *(.text.entry)
    . = ALIGN(4K);
    *(.text.trampoline)
    *(.text.trampoline.*)
    . = ALIGN(4K);
    *(.text .text.*)
  }
  text_end = .;

  . = ALIGN(4K);
  rodata_start = .;
  .rodata : {
    *(.rodata .rodata.*)
  }
  rodata_end = .;

  . = ALIGN(4K);
  data_start = .;
  .data : {
    *(.data .data.*)
  }
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
  bss_end = .;

  . = ALIGN(4K);
  kernel_end = .;

  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame_hdr)
    *(.note .note.*)
  }
}
