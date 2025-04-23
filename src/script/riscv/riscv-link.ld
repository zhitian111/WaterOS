OUTPUT_ARCH(riscv)
ENTRY(_start)
INPUT(wateros_platform_riscv64_gcc.o)
 
LINK_ADDRESS = 0x0000000000200000;
BASE_ADDRESS = 0xffffffc080200000;
KERNAL_OFFSET  = 0xffffffc000000000;
QEMU_ENTRY   = 0x0000000080200000;
SECTIONS
{
    /* . = QEMU_ENTRY; */
    . = BASE_ADDRESS;
    skernel = .;

    stext = .;
    /* .text.entry : AT(QEMU_ENTRY){ */
    /* } */
    .text : AT(QEMU_ENTRY){
        *(.text.entry)
        . = ALIGN(4K);
        *(.text.rust_main)
        . = ALIGN(4K);
        *(.text.usercheck)
        . = ALIGN(4K);
        strampoline = .;
        *(.text.trampoline)
        . = ALIGN(4K);
        etrampoline = .;
        *(.text .text.*)
    }

    . = ALIGN(4K);
    etext = .;
    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    . = ALIGN(4K);
    erodata = .;
    sdata = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    . = ALIGN(4K);
    edata = .;
    sbss_with_stack = .;
    .bss : {
        *(.bss.stack)
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }

    . = ALIGN(4K);
    ebss = .;
    ekernel = .;

    /DISCARD/ : {
        *(.eh_frame)
    }
}
