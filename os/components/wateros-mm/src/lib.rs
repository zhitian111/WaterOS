//! WaterOS 内存管理聚合层：对外 re-export [`api`]（语义契约）、[`frame_alloctor`]（物理帧），
//! 以及经 [`kernel_mm`] 暴露的 bring-up 能力；具体 Sv39/桩代码 **不** 以 `mm_impl` 别名整包导出，避免页表实现细节泄漏到依赖方。
//!
//! ## 页与地址假设
//!
//! - 语义层页大小固定为 **4 KiB**（见 [`api::addr::PAGE_SIZE`]），与 RISC-V Sv39 常用叶子页一致；大页不在本阶段 API 中表达。
//! - [`kernel_mm`] 下 Sv39 实现依赖 **恒等映射或等价的物理线性访问**，以便页表 walk 时把 PPN 当可写指针用；更换映射模型时需同步改 `mm-impl`。
//!
//! ## 与 trap / 执行态的关系
//!
//! - 内核全局页表由 [`kernel_mm`] 路径安装 `satp` 后，trap 与内核代码仍在 **S 态** 下使用同一套映射（见 `kernel_global::init` 文档）；用户态任务切换时再换 `satp`。
//!
//! ## Feature 与桩路径
//!
//! - 默认组合为 `impl-sv39` + 帧 `impl-stack`；仅启用 `impl-dummy`（mm 或 frame）时对应子 crate 的 `//!` 说明当前无操作/固定错误语义，便于在主机或未接线目标上通过编译。

#![no_std]

pub use api_v0 as api;
pub use frame_alloctor;

/// 用户地址空间句柄解析（`LoadedElf::user_aspace_ptr` → 可调用 `HeapBrk` / `MmapOps` 的实例）。
#[cfg(feature = "impl-sv39")]
pub mod user_aspace {
    pub use impl_sv39::user_aspace::*;
}

/// 内核全局页表与用户 ELF 装载；类型契约见 [`api::kernel_bringup`]。
pub mod kernel_mm {
    pub use api_v0::kernel_bringup::{
        DEFAULT_USER_ELF_PATH, LoadElfError, LoadedElf, RootVolumeReadError,
    };

    // 仅依赖 `impl-sv39`：根 crate 的 `qemu-riscv64-opensbi` 不会自动为依赖包打开同名 feature，
    // 若此处再要求 `mm/qemu-riscv64-opensbi`，则从未启用该 flag 的构建会始终落到 dummy，
    // `from_elf_path` 固定返回 `BadClass`（与磁盘/ext4 无关）。
    #[cfg(feature = "impl-sv39")]
    pub use impl_sv39::kernel_mm_impl::{
        ensure_user_execute_for_kernel_va, from_elf_bytes, from_elf_path, init, kernel_satp,
        map_anon_range_user, map_identity_range_user,
    };

    #[cfg(not(feature = "impl-sv39"))]
    pub use impl_dummy::kernel_mm_impl::{
        ensure_user_execute_for_kernel_va, from_elf_path, init, kernel_satp, map_anon_range_user,
        map_identity_range_user,
    };
}

/// 自测入口：`start_ppn`/`end_ppn` 为 **物理页号（PPN）** 闭开区间，供栈式帧分配器初始化；
/// 与 QEMU virt 等 bring-up 传入的可用 RAM 帧范围应对齐（具体由平台传入 `kernel_mm::init` 的区间约定）。
pub fn test_with_range(start_ppn: wateros_base::addr::BasePPN, end_ppn: wateros_base::addr::BasePPN) {
    log::trace!("[wateros-mm] test begin");

    api::test();
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    #[cfg(feature = "impl-sv39")]
    impl_sv39::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[wateros-mm] dummy impl: no mm-impl test");
    }

    log::trace!("[wateros-mm] test end");
}
