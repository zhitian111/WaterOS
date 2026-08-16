# 63 Loongson 2K1000 console 聚合层探针

## 任务内容

静态早期页表生效后，测试 `platform::console` 聚合层是否恢复可用。
在 `P1` 后增加 `CW` 探针。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出出现 `CW`，确认聚合层可用

## 任务简报

- 完成日期：2026-08-16
- console wrapper 探针已加入；等待板端串口。
