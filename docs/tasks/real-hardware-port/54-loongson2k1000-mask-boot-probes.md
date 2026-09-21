# 54 Loongson 2K1000 mask_boot_interrupts 探针

## 任务内容

进入 Rust 后第一条日志已输出。为定位是否卡在 `mask_boot_interrupts`
（尤其是 CRMD/ECFG 写操作），在调用前后增加直写标记 `M0`/`M1`。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出显示是否越过 `M1`

## 任务简报

- 完成日期：2026-08-16
- 已加入 mask_boot_interrupts 探针；等待板端串口。
