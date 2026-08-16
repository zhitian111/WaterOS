# 59 Loongson 2K1000 静态早期页表

## 任务内容

不再在 `_start.S` 手动切 `CRMD.DA`。改为进入 Rust 后先用静态 BSS
页表映射 2K1000 bank1 高 cached RAM，再安装 PGDL 并启用分页。
该页表不依赖 heap/frame allocator/spin lock，避免早期原子操作挂起。

## 涉及文件

- `os/src/early_paging.rs`
- `os/src/main.rs`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出出现 `P0/P1`，并继续后续初始化

## 任务简报

- 完成日期：2026-08-16
- 静态早期页表已实现；等待板端串口。
