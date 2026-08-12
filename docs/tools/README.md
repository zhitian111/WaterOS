# WaterOS 工具与脚本

[项目首页](../../README.md) · [文档总览](../README.md) · [内核工程](../../os/README.md)

本目录是项目工具的稳定入口：说明工具适用的任务、前置条件、推荐调用方式、会修改
哪些文件，以及输出在哪里。具体实现仍以 `os/Makefile` 和 `os/scripts/` 中的脚本为准。

## 按目的选择

- Makefile 设计、参数传播、目标分层与扩展约定：[`makefile.md`](./makefile.md)。
- 脚本选型、直接调用场景与状态影响：[`scripts/README.md`](./scripts/README.md)。
- 完整脚本清单、参数示例与目录规范：[`os/scripts/README.md`](../../os/scripts/README.md)。
- GDB、停滞检测、现场快照和故障注入：[`debugging.md`](./debugging.md)。
- QEMU plugin 的 PC 热点与 WFI 等待时间统计：[`pc-hot.md`](./pc-hot.md)。
- 可重复执行的调试与性能采样步骤：[`../workflows/README.md`](../workflows/README.md)。
- 常规内核构建参数与命令表：仓库根目录 [`README.md`](../../README.md#构建配置)。
- 分阶段 QEMU 测例及日志判读：[`run_testsuits_qemu.md`](../agents/tasks/run_testsuits_qemu.md)
  和 [`analyze_kernel_log.md`](../agents/tasks/analyze_kernel_log.md)。

## 使用约定

优先从 `os/` 运行 `make` 目标；只有 Makefile 未覆盖的场景才直接执行脚本。涉及磁盘
写入的运行必须确认 `SNAPSHOT` / `WRITE_DISK` 设置，并使用副本或 overlay，不能把基准
镜像当作实验盘。工具生成的日志、报告、插件构建物和镜像均不是源码，默认不提交。
