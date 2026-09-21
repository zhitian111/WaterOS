# 55 Loongson 2K1000 runtime 初始化分步探针

## 任务内容

`M0/M1` 已通过，继续在 runtime console/logo/klog/logging/heap
初始化前后加 M2..M11 直写标记，定位首个挂起点。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出显示首个缺失标记

## 任务简报

- 完成日期：2026-08-16
- 分步探针已加入；等待板端串口。
