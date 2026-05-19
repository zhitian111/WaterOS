//! IPC 相关配置常量（pipe、共享内存等子系统的缺省尺度）。

/// 默认 pipe 环形缓冲区容量（字节），供内核内部 pipe 与 `pipe2` 缺省创建路径使用。
pub const DEFAULT_PIPE_CAPACITY: usize = 4096;
