# 57 Loongson 2K1000 早期跳过 runtime console/showlogo

## 任务内容

`M2` 后卡在 `runtime::init_console`。该函数会走 `platform::console`
聚合层，真机早期锁/中断封装尚不可用。首启阶段改为 board console
直写，暂时跳过 `runtime::init_console` 与 `runtime::showlogo`。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出越过 M3/M4

## 任务简报

- 完成日期：2026-08-16
- 已跳过 runtime console/showlogo；等待板端串口。
