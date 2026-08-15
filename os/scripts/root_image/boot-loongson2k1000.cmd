setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.20
setenv loadaddr 0x9000000098000000
tftpboot ${loadaddr} kernel-la2k.ui
bootm ${loadaddr}
