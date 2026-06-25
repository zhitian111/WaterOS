//! 内存布局与 QEMU `virt` 常见假设：DRAM 上界、MMIO 窗口、内核堆尺度。
//!
//! 真机或自定义 `-m` 时应以 DTB/固件为准；此处常量多用于 bring-up 与缺省回退。
//! 堆容量等配置只在本 crate 维护，基础类型包 `wateros-base` 不再复制这些数值。

#[allow(unused)]
/// 内核堆大小的以 2 为底的指数位宽。
/// 降低此值可释放更多物理内存给用户态。
/// 注意：不能设太低，因为 StackFrameAllocator 的 ref_counts Vec (~2MB)
/// 和 allocated Vec (~250KB) 也从内核堆分配。
/// 2^27 = 128MB：全量 benchmark 需比 64MB 更大余量，又避免 256MB 过多占用物理页帧；
/// 降低此值可释放更多物理页帧给用户态。
pub const KERNEL_HEAP_SIZE_BIT_WIDTH : usize = 27;
#[allow(unused)]
/// 内核堆字节容量，即 `1 << KERNEL_HEAP_SIZE_BIT_WIDTH`。
pub const KERNEL_HEAP_SIZE : usize = 1 << KERNEL_HEAP_SIZE_BIT_WIDTH;

/// QEMU `virt` 物理 RAM 起始（包含）。
pub const QEMU_VIRT_PHYS_RAM_BASE : usize = 0x8000_0000;

/// QEMU `virt` 物理 RAM 上界（不包含）：`QEMU_VIRT_PHYS_RAM_BASE + 1GiB`。
/// 与赛题/常见 `-m 1G` 一致；若 DTB 解析失败则作为回退值。
pub const QEMU_VIRT_PHYS_RAM_END : usize = 0xC000_0000;

/// QEMU `virt` 物理 RAM 字节容量。
pub const QEMU_VIRT_PHYS_RAM_SIZE : usize = QEMU_VIRT_PHYS_RAM_END - QEMU_VIRT_PHYS_RAM_BASE;

/// QEMU `virt` 低地址 MMIO 恒等映射区间（半开）：UART、`virtio,mmio`
/// 等外设所在物理地址。 与 OpenSBI/QEMU 设备树常见布局一致；**不是** DRAM，扩大
/// RAM 映射无法替代。
pub const QEMU_VIRT_MMIO_PHYS_START : usize = 0x1000_0000;
/// QEMU `virt` MMIO 区间上界（不包含）；与 `QEMU_VIRT_MMIO_PHYS_START`
/// 组成半开区间。
pub const QEMU_VIRT_MMIO_PHYS_END : usize = 0x1200_0000;
