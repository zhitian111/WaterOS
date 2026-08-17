//! WaterOS 内存管理聚合层：对外 re-export
//! [`api`]（语义契约）、[`frame_alloctor`]（物理帧）， 以及经 [`kernel_mm`]
//! 暴露的 bring-up 能力；具体 Sv39/LoongArch64/桩代码 **不** 以 `mm_impl`
//! 别名整包导出，避免页表实现细节泄漏到依赖方。
//!
//! ## 页与地址假设
//!
//! - 语义层页大小固定为 **4 KiB**（见 [`api::addr::PAGE_SIZE`]），与 RISC-V
//!   Sv39 常用叶子页一致；大页不在本阶段 API 中表达。
//! - [`kernel_mm`] 下各 impl 依赖 **恒等映射或等价的物理线性访问**，以便页表
//!   walk 时把 PPN 当可写指针用；更换映射模型时需同步改 `mm-impl`。
//!
//! ## Feature 与桩路径
//!
//! - 默认组合为中性 `api-v0`；`impl-sv39` 用于 RISC-V，`impl-loongarch64` 用于
//!   LoongArch， 通过根 crate feature chain 选择。
//! - 两者互斥：Cargo features 不支持在依赖链上去重，但本文件通过 `cfg`
//!   确保仅一个 impl 的符号被编译进当前 crate。

#![no_std]

pub use api_v0 as api;
pub use frame_alloctor;
#[doc(hidden)]
pub use impl_common::load_or_get_readonly_mmap_page;

pub mod mempolicy;

// ── 互斥的 impl 选择 ────────────────────────────────────────────
// 三个 impl 各提供同名的模块与类型，但它们通过 `optional` 依赖 + feature flag
// 被条件编译， 因此只要不两条 feature chain 同时激活，就不会冲突。

#[cfg(feature = "impl-loongarch64")]
use impl_loongarch64 as active_mm_impl;
#[cfg(feature = "impl-sv39")]
use impl_sv39 as active_mm_impl;

/// 用户地址空间句柄解析（`LoadedElf::user_aspace_ptr` → 可调用 `HeapBrk` /
/// `MmapOps` 的实例）。
pub use active_mm_impl::user_aspace;

/// 用户缓冲区访问（syscall 路径）。
pub mod user_access {
    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::user_access::LoongArch64UserMemoryOps;
    #[cfg(feature = "impl-sv39")]
    pub use impl_sv39::user_access::Sv39UserMemoryOps;
    pub use super::active_mm_impl::user_access::{debug_probe_user_virt, UserVirtProbe};
}

/// 当前活动架构的用户缓冲区实现类型别名。
#[cfg(feature = "impl-sv39")]
pub type ActiveUserMemoryOps = impl_sv39::user_access::Sv39UserMemoryOps;
#[cfg(feature = "impl-loongarch64")]
pub type ActiveUserMemoryOps = impl_loongarch64::user_access::LoongArch64UserMemoryOps;

/// 内核全局页表与用户 ELF 装载；类型契约见 [`api::kernel_bringup`]。
pub mod kernel_mm;
/// 自测入口：`start_ppn`/`end_ppn` 为 **物理页号（PPN）**
/// 闭开区间，供栈式帧分配器初始化； 与 QEMU virt 等 bring-up 传入的可用 RAM
/// 帧范围应对齐（具体由平台传入 `kernel_mm::init` 的区间约定）。
pub fn test_with_range(start_ppn : api::addr::PhysPageNum,
                       end_ppn : api::addr::PhysPageNum) {
    log::trace!("[wateros-mm] test begin");

    api::test();
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    #[cfg(feature = "impl-sv39")]
    impl_sv39::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-loongarch64")]
    impl_loongarch64::test_with_range(start_ppn, end_ppn);

    log::trace!("[wateros-mm] test end");
}

/// 用户写入进度契约与两套页表实现的定向自测。
///
/// 依赖已经初始化的物理帧分配器；测试结束后会释放临时地址空间和数据页。
pub fn test_user_copy_progress() {
    api::user_access::test();
    impl_sv39::user_access::test_copy_to_user_progress();
    impl_loongarch64::user_access::test_copy_to_user_progress();
}

/// 启动后 MM 统一入口；架构实现负责建立内核地址空间和帧分配器。
pub fn init_after_boot(dtb_pa: usize, memory_end: usize) {
    log::info!("[mm] init_after_boot begin");
    kernel_mm::init(dtb_pa, memory_end);
    log::info!("[mm] init_after_boot complete");
}

/// Idle task 的有界 MM 维护入口。当前仅补充全局预清零 frame 池；具体实现保证
/// allocator 锁忙时立即返回，且不在任何锁内清零页面。
pub fn idle_maintenance() {
    frame_alloctor::idle_zeroed_frame_pool_maintenance();
}

#[cfg(feature = "self_test")]
/// MM 组件统一自检入口；不自行猜测物理内存范围，避免破坏已初始化的帧池。
pub fn self_test() {
    log::info!("[mm] self_test begin");
    api::test();
    frame_alloctor::self_test();
    impl_common::self_test();
    #[cfg(all(feature = "impl-sv39", target_arch = "riscv64"))]
    impl_sv39::self_test();
    #[cfg(all(feature = "impl-loongarch64", target_arch = "loongarch64"))]
    impl_loongarch64::self_test();
    log::info!("[mm] self_test complete; temporary mappings and frames were reclaimed");
}
