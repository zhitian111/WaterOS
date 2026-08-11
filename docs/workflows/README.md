# WaterOS 标准操作流程

本目录把常用的多步骤操作沉淀为可复用流程。工具参数和实现细节仍以
[`../tools/`](../tools/README.md) 为准；执行任务时先选择本目录的流程，再按需要查阅工具说明。

- [`debugging.md`](./debugging.md)：复现卡死、捕获现场、符号化并形成最小证据包。
- [`pc-hot.md`](./pc-hot.md)：用 PC-hot 与 wait-hot 采样性能热点，进行可比分析。

所有涉及 QEMU 磁盘写入的流程都必须使用 snapshot、overlay 或镜像副本；输出的日志、
报告和插件构建物不提交到仓库。
