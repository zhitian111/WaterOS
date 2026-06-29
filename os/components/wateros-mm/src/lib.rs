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
    #[cfg(feature = "impl-sv39")]
    pub use impl_sv39::user_access::{debug_probe_user_virt, UserVirtProbe};
}

/// 当前活动架构的用户缓冲区实现类型别名。
#[cfg(feature = "impl-sv39")]
pub type ActiveUserMemoryOps = impl_sv39::user_access::Sv39UserMemoryOps;
#[cfg(feature = "impl-loongarch64")]
pub type ActiveUserMemoryOps = impl_loongarch64::user_access::LoongArch64UserMemoryOps;

/// 内核全局页表与用户 ELF 装载；类型契约见 [`api::kernel_bringup`]。
pub mod kernel_mm {
    pub use api_v0::kernel_bringup::{
        LoadElfError, LoadProgramError, LoadedElf, PrepareUserStackError, RootVolumeReadError, DEFAULT_USER_ELF_PATH,
    };
    pub use api_v0::executable::ExecResolveError;

    /// 在已装载 ELF 的用户栈上写入 argc/argv/envp/auxv，返回初始 `sp`。
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    pub fn prepare_elf_user_stack(
        elf: &LoadedElf,
        argv: &[&str],
        envp: &[&str],
    ) -> Result<usize, PrepareUserStackError> {
        let ops = crate::ActiveUserMemoryOps::new(elf.user_aspace_ptr);
        api_v0::elf_user_stack::prepare_elf_user_stack(&ops, elf, argv, envp)
    }

    #[cfg(feature = "impl-sv39")]
    pub use impl_sv39::kernel_mm_impl::{
        drop_user_aspace, ensure_user_execute_for_kernel_va, fork_user_aspace, from_elf_bytes,
        from_elf_path, init, kernel_satp, load_program_from_path, map_anon_range_user,
        map_identity_range_user,
    };

    pub fn handle_cow_fault(aspace_ptr: usize, fault_addr: usize) -> bool {
        #[cfg(feature = "impl-sv39")]
        {
            return impl_sv39::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr)
                .unwrap_or(false);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return impl_loongarch64::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr)
                .unwrap_or(false);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr);
            false
        }
    }

    pub fn handle_user_page_fault(
        aspace_ptr: usize,
        fault_addr: usize,
        access: api_v0::mmap::PageFaultAccess,
    ) -> bool {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                MmapOps::handle_page_fault(aspace, &mut alloc, VirtAddr(fault_addr), access)
            })
            .unwrap_or(false);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                MmapOps::handle_page_fault(aspace, &mut alloc, VirtAddr(fault_addr), access)
            })
            .unwrap_or(false);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr, access);
            false
        }
    }

    pub fn madvise_discard_pages(aspace_ptr: usize,
                                 addr: usize,
                                 len: usize)
                                 -> api_v0::error::MmResult<()> {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                aspace.madvise_discard_mapped_pages(&mut alloc, VirtAddr(addr), len)
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                aspace.madvise_discard_mapped_pages(&mut alloc, VirtAddr(addr), len)
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, addr, len);
            Ok(())
        }
    }

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::kernel_mm_impl::{
        drop_user_aspace, ensure_user_execute_for_kernel_va, fork_user_aspace, from_elf_bytes,
        from_elf_path, init, kernel_satp, load_program_from_path, map_anon_range_user,
        map_identity_range_user,
    };

    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    pub use impl_dummy::kernel_mm_impl::{
        ensure_user_execute_for_kernel_va, fork_user_aspace, from_elf_path, init, kernel_satp,
        load_program_from_path, map_anon_range_user, map_identity_range_user,
    };
}

/// 自测入口：`start_ppn`/`end_ppn` 为 **物理页号（PPN）**
/// 闭开区间，供栈式帧分配器初始化； 与 QEMU virt 等 bring-up 传入的可用 RAM
/// 帧范围应对齐（具体由平台传入 `kernel_mm::init` 的区间约定）。
pub fn test_with_range(start_ppn : wateros_base::addr::BasePPN,
                       end_ppn : wateros_base::addr::BasePPN) {
    log::trace!("[wateros-mm] test begin");

    api::test();
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    #[cfg(feature = "impl-sv39")]
    impl_sv39::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-loongarch64")]
    impl_loongarch64::test_with_range(start_ppn, end_ppn);
    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[wateros-mm] dummy impl: no mm-impl test");
    }

    log::trace!("[wateros-mm] test end");
}
