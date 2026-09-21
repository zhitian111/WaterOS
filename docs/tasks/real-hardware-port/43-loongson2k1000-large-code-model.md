# 43 Loongson 2K1000 使用 large code model

## 任务内容

`WR` 探针说明汇编已到跳转 Rust 前，但 Rust 入口仍未输出。2K1000
由 U-Boot 在高 cached 段（VSEG=9）执行，而 LoongArch 默认 small
code model 生成的 `pcalau12i` 等 32 位 PC 相对寻址无法覆盖该地址
空间。

本任务为 `loongson2k1000la` 目标固定 `-C code-model=large`。

## 涉及文件

- `os/Makefile`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] uImage 增大到约 3 MiB（large model 引入更多 GOT 加载）
- [x] 新内核已更新到 TFTP
- [ ] 板端输出 `WR[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- large code model 已启用；等待板端串口。
