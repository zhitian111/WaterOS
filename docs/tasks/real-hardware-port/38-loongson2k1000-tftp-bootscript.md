# 38 Loongson 2K1000 TFTP 启动脚本

## 任务内容

为 2K1000 板端提供一键网络启动脚本，减少 U-Boot 手敲命令：

1. 新增 `os/scripts/root_image/boot-loongson2k1000.cmd`
2. 新增 `make la2k_bootscr` 生成 legacy script uImage
3. 脚本放到 TFTP root，板端用 `tftpboot + source` 执行

## 脚本内容

```text
setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.20
setenv loadaddr 0x9000000098000000
tftpboot ${loadaddr} kernel-la2k.ui
bootm ${loadaddr}
```

## 涉及文件

- `os/scripts/root_image/boot-loongson2k1000.cmd`
- `os/Makefile`

## 验收方式

- [x] `make la2k_bootscr` 生成 `build/wateros-2k1000.scr`
- [x] TFTP 本地拉取 `wateros-2k1000.scr` 成功
- [ ] 板端 `tftpboot + source` 后能自动下载并 `bootm` WaterOS

## 板端命令

```text
setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.20
tftpboot 0x9000000091000000 wateros-2k1000.scr
source 0x9000000091000000
```

## 任务简报

- 完成日期：2026-08-16
- 宿主侧生成与 TFTP 验证通过；板端实际 `source`/`bootm` 仍待串口日志。
