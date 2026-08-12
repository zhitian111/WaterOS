# runtime-logging

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

本 crate 将 `log` facade 接到 `runtime-console`。`init()` 根据 `impl-trace` 至
`impl-error` feature 选择最大等级；多个 feature 同时出现时选择最详细的等级。

初始化必须在 platform console 可写后执行，且只应执行一次。logger 是静态无状态对象，
输出路径依赖 console；不要在 allocator、console 锁或 panic 的敏感路径中产生会再次
写日志的回调。

日志记录包含当前 CPU 标签，仅用于诊断；它不是 scheduler current-task 或 online 状态。
