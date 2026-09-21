# 58 Loongson 2K1000 早期 DA 切换并刷新 TLB/指令屏障

## 任务内容

回答“是否进入 Rust 前已建立页表”：当前 WaterOS 并非如此；`kernel_mm::init`
在 Rust 启动流程后段才执行。真机 U-Boot 跳转时仍处于 `PG=1/DA=0`，
导致早期高 cached 数据原子操作与 console 锁失效。

本任务在 `_start.S` 尽早切到 `CRMD.DA=1/PG=0`，并在写 CSR 后执行
`invtlb` 和 `ibar`，避免之前只写 CRMD 导致取指挂起。

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出越过 `B`，后续原子/console 不再卡住

## 任务简报

- 完成日期：2026-08-16
- 早期 DA 切换 + TLB flush 已加入；等待板端串口。
