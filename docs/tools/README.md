# WaterOS 工具与脚本

本目录是项目工具的稳定入口：说明工具适用的任务、前置条件、推荐调用方式、会修改
哪些文件，以及输出在哪里。具体实现仍以 `os/Makefile` 和 `os/scripts/` 中的脚本为准。

## 按目的选择

- 构建、配置、镜像选择、QEMU 启动、并行运行和脚本清单：[`scripts/README.md`](./scripts/README.md)。
- GDB、停滞检测、现场快照和故障注入：[`debugging.md`](./debugging.md)。
- QEMU plugin 的 PC 热点与 WFI 等待时间统计：[`pc-hot.md`](./pc-hot.md)。
- 可重复执行的调试与性能采样步骤：[`../workflows/README.md`](../workflows/README.md)。
- 常规内核构建和运行目标：[`os/README.md`](../../os/README.md) 与 `os/Makefile`。
- 分阶段 QEMU 测例及日志判读：[`run_testsuits_qemu.md`](../agents/tasks/run_testsuits_qemu.md)
  和 [`analyze_kernel_log.md`](../agents/tasks/analyze_kernel_log.md)。

## 使用约定

优先从 `os/` 运行 `make` 目标；只有 Makefile 未覆盖的场景才直接执行脚本。涉及磁盘
写入的运行必须确认 `SNAPSHOT` / `WRITE_DISK` 设置，并使用副本或 overlay，不能把基准
镜像当作实验盘。工具生成的日志、报告、插件构建物和镜像均不是源码，默认不提交。
