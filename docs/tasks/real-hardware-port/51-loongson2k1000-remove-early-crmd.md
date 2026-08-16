# 51 Loongson 2K1000 取消入口早期 CRMD 切换

## 任务内容

探针显示输出停在 `WA`，即 `csrwr CRMD` 前后卡住。说明在 U-Boot 的
高 cached 段执行过程中直接改写 `CRMD.DA/PG` 会破坏当前取指/地址翻译。
因此移除 `_start.S` 中的早期 CRMD 切换，保留高地址链接和页表映射，
继续观察高链接方案本身。

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出继续到 `WRS[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 已移除早期 CRMD 改写；等待板端串口。
