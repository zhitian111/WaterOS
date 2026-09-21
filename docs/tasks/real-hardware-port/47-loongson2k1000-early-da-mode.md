# 47 Loongson 2K1000 入口早期切换到直接地址翻译

## 任务内容

板端在读取高 cached 段数据时触发 `TLB exception`。根因是 U-Boot 跳转
前 `CRMD.PG=1/DA=0`；在尚未建立内核页表前，高 VSEG=9 数据访问不能
保证走 DMW，因而触发 TLB refill。

本任务在 `_start.S` 早期把 `CRMD` 设为 `DA=1, PG=0`，让 DMW
直接地址翻译窗口立即生效。

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端不再出现 `TLB exception`，继续输出 `WRS[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 早期 DA 模式切换完成；等待板端串口。
