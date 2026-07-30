//! IPC 相关配置常量（pipe、共享内存等子系统的缺省尺度）。

/// 默认 pipe 环形缓冲区容量（字节），供内核内部 pipe 与 `pipe2` 缺省创建路径使用。
///
/// Linux 通常以 16 个 4 KiB 页作为新 pipe 的初始容量。保持相同量级可避免编译器等
/// 生产者在消费者短暂调度延迟时过早阻塞，同时 `PIPE_BUF` 的原子写语义仍由 4 KiB
/// 上限单独约束，不应与总容量混为一谈。
pub const DEFAULT_PIPE_CAPACITY : usize = 64 * 1024;
