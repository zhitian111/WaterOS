setenv loadaddr 0x9000000098000000
setenv fdt_addr 0x9000000092000000
setenv fdt_addr_r 0x9000000092000000
setenv fdt_high 0xffffffffffffffff
scsi reset
scsi dev 0
fatload scsi 0:1 ${loadaddr} kernel-la2k.bin
fatload scsi 0:1 ${fdt_addr_r} loongson2k1000.dtb
go ${loadaddr} ${fdt_addr_r}
