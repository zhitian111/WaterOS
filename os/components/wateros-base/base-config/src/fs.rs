//! 根卷文件 I/O 与页缓存、块设备 LBA 缓存的 bring-up 尺度常量。
//!
//! VFS 大文件分流、全局页缓存与 `impl-block-cache` 装饰器均从此处读取默认值。

/// 文件页缓存行大小（字节）；read/write 按该粒度加载与淘汰。
pub const FILE_PAGE_SIZE : usize = 4096;

/// 全局页帧 LRU 槽位数（所有文件共享，非每文件容量）。
/// 4096 * 4KiB = 16MiB，覆盖多数测试热集。内核堆共 128MiB，
/// 需为 ELF 加载、VFS 元数据、进程控制块等留出充足内存。
pub const FILE_PAGE_CACHE_CAPACITY : usize = 4096;

/// Direct 模式下顺序读预取步长（页数）；`0` 表示关闭预取。
/// 增大此值可减少读时的缺页中断次数，对顺序读性能有明显提升。
/// 4~8 是 lmbench/unixbench 场景的合理值：预取 8×4KiB = 32KiB，与 ext4 常用读粒度一致。
pub const FILE_READ_AHEAD_STRIDE : usize = 8;

/// 根卷文件 I/O 模式：v1 仅 [`FileIoMode::Direct`] 有实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIoMode {
    /// 同步 syscall 路径：命中页缓存或下钻 FS `read_range` / `write_range`。
    Direct,
    /// 异步提交队列（v1 未实现，调用方应返回 `Unsupported`）。
    Async,
}

/// 当前启用的根卷文件 I/O 模式。
pub const FILE_IO_MODE : FileIoMode = FileIoMode::Direct;

/// `CachingBlockDevice` 可缓存的逻辑块数量；`0` 表示注册时不包装缓存。
/// 1024 × 512B = 512KiB；QEMU 1G RAM / 内核堆 128MiB 下可承受单 virtio-blk 槽预分配。
pub const BLOCK_CACHE_CAPACITY_BLOCKS : usize = 1024;
