# runtime-console

控制台分为三层：`console-api/api-v0` 定义 `Console` 后端约束，`console-impl/*`
连接 dummy 或 platform 后端，根 crate 提供统一输出 API 和宏。

调用方使用 `write_fmt`、`write_str`、`write_raw_bytes` 或 `print!`/`println!`；不要直接
构造 backend handle。`impl-platform-console` 时整条格式化输出进入 platform 的跨 CPU
控制台锁；`impl-dummy` 只用于暴露错误配置，不能用于真实运行。两个 impl feature
互斥。

`write_raw_bytes` 用于 stdout/syscall 等非 UTF-8 数据，调用方应按缓冲区而非单字节写，
避免增加锁竞争和串口 MMIO 循环。
