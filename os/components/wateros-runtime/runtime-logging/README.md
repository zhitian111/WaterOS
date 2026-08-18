# runtime-logging

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

本 crate 把 `log` facade 接到 runtime-console，提供编译期等级裁剪和彩色单行输出。它不保存 ring buffer、不异步落盘，也没有动态调级接口。

## feature 与初始化

`impl-trace/debug/info/warn/error` 最多启用一个，组合由 compile_error 拒绝；未启用任何等级时为 Off，`init()` 不注册 logger。等级 feature 同时转发 `log/max_level_*`，高于上限的宏和参数表达式在编译期裁掉，因此不要依赖日志参数中的副作用。

调用顺序：platform console 可写 → runtime-console root 安装 → logging::init → 普通模块日志。内部 `logger::init` 先 `log::set_logger(&LOGGER)`，成功后设置 max level，再打印初始化消息。

crate 根 `init()` 故意丢弃 `SetLoggerError`。第二次初始化通常静默失败并保留原 logger/level，无法用返回值诊断；BSP 必须保证一次调用。比赛中若需要可测初始化，新增 `try_init() -> Result`，保留旧 `init()` 兼容包装，而不是反复重试。

## 过滤与格式

`enabled` 比较 record level 与 `STATIC_MAX_LEVEL`，并特别屏蔽 target 以 `ext4_rs` 开头且 level≥Info 的记录。按 `log::Level` 排序语义，这条条件的实际覆盖范围应通过测试确认；修改过滤时不要凭“严重程度文字”猜枚举比较方向。

输出格式为彩色：`[WaterOS][cpu=N] [LEVEL] message`。一条 record 只调用一次 console `println!`，减少多 CPU 字段交错；真正原子性由 runtime-console 的整段写锁提供。终端不支持 ANSI 时会看到转义序列，目前没有 no-color 开关。

CPU label 在启用 platform-console feature 时读取 arch current CPU id，否则固定 0。它不是 task id，也不保证该 CPU 已被 scheduler 标 online；早期 BSP/AP 日志要结合启动阶段解释。

## 锁、递归与故障路径

`WaterOSLogger` 自身无锁、无字段、`flush()` 为空，所有同步下沉 console。`record.args()` 的 formatting 在 console 写路径执行，可能调用用户自定义 Display；禁止 Display 再打日志、拿反向锁或分配可能失败的 heap。

以下位置应避免普通日志：持 console 锁、frame/heap allocator 内部关键锁、panic logger 重入、关中断且有严格时限的 trap、持 VFS/device 锁而 console 路径可能反向访问它们。诊断这些位置应使用经过证明的 raw emergency console 或无分配计数器。

logger 不缓冲，flush 空实现不提供“日志已持久化”保证。reset/panic 前调用 `log::logger().flush()` 没有额外效果；需要串口 TEMT 则显式调用 platform/runtime console flush。

## 扩展示例：运行期调级

若新增动态 level，只能在编译期 STATIC_MAX_LEVEL 以内调低/恢复；被编译裁掉的 Trace 无法运行时打开。使用原子 `LevelFilter` 或 log 自带 max level，明确 SMP ordering，并保留 ext4 target filter。提供 proc/sysctl 写入口时先做权限和字符串解析，不能让用户态替换全局 logger。

## 回归清单

- 每个互斥 feature 单独构建，任意两档组合编译失败，无 feature 时无输出；
- 运行 level 边界及日志参数副作用确实被静态裁剪；
- ext4 target 与其它 target、各 level 的过滤矩阵；
- BSP/AP CPU label、无 platform-console 时固定 0；
- 多 CPU 长消息每条不互插，ANSI 开始/清除完整；
- 重复 init 保持首个 logger，新增 try_init 时返回可见错误；
- formatter 递归日志、console 写失败、panic/allocator 敏感路径不死锁；
- reset 前显式 console flush 的真实输出完成语义。
