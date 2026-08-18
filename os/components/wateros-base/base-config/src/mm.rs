//! 内存布局与 QEMU `virt` 常见假设：DRAM 上界、MMIO 窗口、内核堆尺度。
//!
//! 真机或自定义 `-m` 时应以 DTB/固件为准；此处常量多用于 bring-up 与缺省回退。
//! 堆容量等配置只在本 crate 维护，`wateros-base` 不再复制这些数值。

/// 内核堆大小的以 2 为底的指数位宽。
///
/// 降低此值可释放更多物理内存给用户态。不能设太低：`StackFrameAllocator` 的
/// `ref_counts` 与 `allocated` Vec 随平台公布的物理页数增长，也从内核堆分配。
/// 全量 native Rust build 已观测到约 121MB 活跃分配；128MB 池会因分配器元数据
/// 和碎片余量不足而向用户态返回 `ENOMEM`。2026-08-15：stress-ng `--forkheavy`
/// 等内存/进程类压力在 256MB 池下触发 `[heap] OOM`（used≈248MB，分配 1MB 失败）
/// 并 panic（final=8G RAM 场景）。提升到 512MB 以覆盖内存类压力测试；注意 LA
/// pre 仅 1G RAM 时 512MB 堆会挤占用户内存（当前策略：final 能跑优先）。
pub const KERNEL_HEAP_SIZE_BIT_WIDTH: usize = 29;
/// 内核堆字节容量，即 `1 << KERNEL_HEAP_SIZE_BIT_WIDTH`。
/// 位宽必须小于目标指针宽度；否则移位会溢出并在编译期拒绝构建。
pub const KERNEL_HEAP_SIZE: usize = 1 << KERNEL_HEAP_SIZE_BIT_WIDTH;

/// QEMU `virt` 物理 RAM 起始（包含）。
pub const QEMU_VIRT_PHYS_RAM_BASE: usize = 0x8000_0000;

/// QEMU `virt` 物理 RAM 上界（不包含）：`QEMU_VIRT_PHYS_RAM_BASE + 1GiB`。
/// 与赛题/常见 `-m 1G` 一致；若 DTB 解析失败则作为回退值。
pub const QEMU_VIRT_PHYS_RAM_END: usize = 0xC000_0000;

/// QEMU `virt` 物理 RAM 字节容量。
pub const QEMU_VIRT_PHYS_RAM_SIZE: usize = QEMU_VIRT_PHYS_RAM_END - QEMU_VIRT_PHYS_RAM_BASE;

/// 链接器预留的 VirtIO DMA 池大小（字节）。该区域不交给普通帧分配器，专用于
/// 需要物理连续地址的队列和暂存缓冲；大小为零会使设备初始化无法建立队列。
pub const DMA_POOL_SIZE: usize = 16 * 1024 * 1024;

/// QEMU RISC-V `virt` 上 Goldfish RTC 的物理 MMIO 页（半开区间）。
/// RTC 位于常规 UART/VirtIO MMIO 窗口之外，因此需要单独恒等映射。
pub const QEMU_VIRT_RTC_PHYS_START: usize = 0x0010_1000;
pub const QEMU_VIRT_RTC_PHYS_END: usize = 0x0010_2000;

/// QEMU `virt` 低地址 MMIO 恒等映射区间（半开）：UART、`virtio,mmio` 等外设所在物理地址。
/// 与 OpenSBI/QEMU 设备树常见布局一致；**不是** DRAM，扩大 RAM 映射无法替代。
pub const QEMU_VIRT_MMIO_PHYS_START: usize = 0x1000_0000;
/// QEMU `virt` MMIO 区间上界（不包含）。
pub const QEMU_VIRT_MMIO_PHYS_END: usize = 0x1200_0000;
