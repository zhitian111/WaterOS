# 60 Loongson 2K1000 首启跳过 heap init

## 任务内容

静态早期页表生效后，启动推进到 M10，但卡在
`runtime::heap_allocator::init`。先跳过该步骤，确认后续
arch/platform 初始化链路；heap init 单独定位。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出越过 M11

## 任务简报

- 完成日期：2026-08-16
- 已临时跳过 heap init；等待板端串口。
