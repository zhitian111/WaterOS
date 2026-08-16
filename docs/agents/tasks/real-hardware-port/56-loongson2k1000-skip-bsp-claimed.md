# 56 Loongson 2K1000 首启跳过 BSP_CLAIMED 原子操作

## 任务内容

探针显示 `M1` 后未到 `M2`，中间只有 `BSP_CLAIMED.swap`。真机早期高
cached 数据原子操作前尚未建立页表，会挂住。首启阶段 SMP AP 尚不支持，
先移除 2K1000 入口的 BSP_CLAIMED 原子占位，待 MMU 初始化后再恢复。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出继续越过 `M2`

## 任务简报

- 完成日期：2026-08-16
- 已跳过首启 BSP_CLAIMED 原子操作；等待板端串口。
