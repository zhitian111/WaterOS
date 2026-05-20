//! 根卷文件 I/O 与页缓存、块设备 LBA 缓存的 bring-up 尺度常量。
//!
//! VFS 大文件分流、全局页缓存与 `impl-block-cache` 装饰器均从此处读取默认值。

/// 文件页缓存行大小（字节）；read/write 按该粒度加载与淘汰。
pub const FILE_PAGE_SIZE: usize = 4096;

/// 超过该大小的普通文件在 VFS `open` 时走页缓存句柄，不再整文件读入 RAM。
pub const FILE_LARGE_THRESHOLD: u64 = 64 * 1024;

/// 全局页帧 LRU 槽位数（所有文件共享，非每文件容量）。
pub const FILE_PAGE_CACHE_CAPACITY: usize = 32;

/// Direct 模式下顺序读预取步长（页数）；`0` 表示关闭预取。
pub const FILE_READ_AHEAD_STRIDE: usize = 1;

/// 根卷文件 I/O 模式：v1 仅 [`FileIoMode::Direct`] 有实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIoMode {
    /// 同步 syscall 路径：命中页缓存或下钻 FS `read_range` / `write_range`。
    Direct,
    /// 异步提交队列（v1 未实现，调用方应返回 `Unsupported`）。
    Async,
}

/// 当前启用的根卷文件 I/O 模式。
pub const FILE_IO_MODE: FileIoMode = FileIoMode::Direct;

/// `CachingBlockDevice` 可缓存的逻辑块数量；`0` 表示注册时不包装缓存。
pub const BLOCK_CACHE_CAPACITY_BLOCKS: usize = 64;
