# Loongson 2K1000 镜像/TFTP 工具

日常入口统一由 `os/Makefile` 提供。`make la2k_image` 通过 `user/tools` 生成 GPT 整盘镜像；
本目录只实现镜像分片、U-Boot 烧录脚本生成和 TFTP 服务。

```sh
make la2k_tftp_prepare
make la2k_tftp LA2K_TFTP_SERVER_IP=192.168.1.2 \
  LA2K_TFTP_BOARD_IP=192.168.1.20
```

`la2k_tftp_prepare` 清理并填充 `os/build/la2k-tftp`，把镜像按默认 32 MiB 分片并生成
`wateros-2k1000-flash.scr`，但不启动服务。`la2k_tftp` 随后以前台方式启动 dnsmasq；
TFTP 使用 `--port=0`，不会启动 DHCP 或 DNS。主机必须已把服务地址配置到直连网卡。

脚本启动后，在板子 U-Boot 输入：

```text
setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.20
tftpboot 0x9000000091000000 wateros-2k1000-flash.scr
source 0x9000000091000000
```

脚本从 LBA 0 开始逐片覆盖 `scsi 0`，写完后执行 `saveenv`，保存从 GPT FAT P1
加载并 `source boot.scr` 的 `bootcmd`。`boot.scr` 再加载 `kernel-la2k.bin` 与 DTB，并通过
`go kernel dtb` 进入内核。执行前必须确认 `scsi 0` 是目标 SATA 盘且其原数据可以被覆盖。

需要直接调用实现脚本时，先运行 `prepare_tftp.sh --help`；镜像大小必须是 512 字节的整数倍。
