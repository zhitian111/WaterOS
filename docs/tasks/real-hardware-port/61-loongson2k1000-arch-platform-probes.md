# 61 Loongson 2K1000 arch/platform 初始化探针

## 任务内容

M11 已越过，继续在 `init_current_cpu`、`arch::init`、`init_ipi`、
`init_when_boot`、`init_configured_cpu_mask`、`init_after_boot`、
`set_cpu_online` 前后加 M12..M20 探针。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出显示首个缺失标记

## 任务简报

- 完成日期：2026-08-16
- 分步探针已加入；等待板端串口。
