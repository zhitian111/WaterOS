setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.20
setenv flash_loadaddr 0x9000000090000000
tftpboot ${flash_loadaddr} wateros-la.img
setexpr flash_blocks ${filesize} + 511
setexpr flash_blocks ${flash_blocks} / 512
scsi dev 0
scsi write ${flash_loadaddr} 0 ${flash_blocks}
echo WaterOS SATA image written
