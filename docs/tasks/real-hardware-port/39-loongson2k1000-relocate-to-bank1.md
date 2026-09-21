# 39 Loongson 2K1000 内核地址改到 U-Boot 安全加载区

## 任务内容

首轮 TFTP `bootm` 在进入 WaterOS 前，U-Boot 的 Loongson bootparam
路径触发 `CPU0 exception`。定位后确认 2K1000 的 U-Boot 在物理
`0x9000_0000` 起保留了 256 KiB 的 lock-cache 区，而我们原先把内核
load/entry 放在 `0x9000_0000`，会覆盖该区域。

本任务把 2K1000 内核移到 U-Boot 默认加载区：

1. linker entry 改为 `0x9800_0000`
2. uImage load/entry 改为 `0x9800_0000`
3. 平台 RAM fallback 从 `0x9000_0000..0xa000_0000` 扩大到
   `0x9000_0000..0xc000_0000`（完整 bank1，U-Boot `bdinfo` 报告）

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/linker/link.ld`
- `os/Makefile`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/memory.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] uImage header `load=entry=0x98000000`, `arch=27`
- [x] TFTP 文件更新为当前内核
- [ ] 板端再次 `tftpboot`/`bootm` 进入 WaterOS，无 `CPU0 exception`

## 任务简报

- 完成日期：2026-08-16
- 宿主侧验证通过；真机第二轮启动待串口日志。
