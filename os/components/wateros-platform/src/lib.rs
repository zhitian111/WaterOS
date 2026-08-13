#![no_std]

//! WaterOS **平台聚合**：把 `arch`（指令集 / CSR 原语）与 `platform-impl`
//!（板级或 QEMU 等具体环境）组合成内核可直接调用的入口。
//!
//! ## 分层与边界
//! - **`arch`（`wateros-platform-arch`）**：与 ISA 强相关的最小原语（trap 上下文、
//!   `time` CSR、中断屏蔽位、`satp` 等）。
//! - **`platform-impl`**：当前板级 profile，负责 boot 参数、时间频率、early console、
//!   deadline timer、reset 等平台能力。后端可以是 OpenSBI、MMIO UART 或其它机制。
//! - **本 crate**：在以上两层之上提供**组合语义**（例如 [`timer`] 将 arch 的 tick
//!   与平台 deadline timer 编程衔接），同时保存与机器无关的 IPI reason。
//!
//! 归属规则：本地 CSR、汇编和 trap 帧应落在 [`arch`]；SBI、DTB、QEMU machine
//! 约定和设备地址应落在 `platform-impl`；调度决策不能放入这两层。
//!
//! 默认 feature 与成员 crate 对应关系以根目录 `Cargo.toml` 为准；文档描述的是各层
//! **语义契约**与替换点，而非单一硬件路径的实现细节。

/// 当前 feature 选中的板级实现 crate（LoongArch QEMU virt）。
#[cfg(feature = "impl-qemu-loongarch64-virt")]
pub use impl_qemu_loongarch64_virt as active_impl;
/// 当前 feature 选中的板级实现 crate（RISC-V QEMU + OpenSBI）。
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi as active_impl;

/// 引导早期：保存平台持有的 DTB 物理指针（内存布局等平台侧解析使用）。
pub fn init_when_boot(dtb_pa: usize) {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::dtb::store(dtb_pa);
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::dtb::store(dtb_pa);
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt")))]
    {
        let _ = dtb_pa;
    }
}

/// 启动后平台阶段入口；平台 profile 的 DTB/时间基础已在 boot 阶段保存。
pub fn init_after_boot() {
    log::info!("[platform] init_after_boot: platform services ready");
}

/// 平台持有的 DTB 物理指针（未保存时为 0）。
pub fn dtb_pa() -> usize {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        impl_qemu_riscv64_opensbi::dtb::dtb_pa()
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::dtb::dtb_pa()
    }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt")))]
    {
        0
    }
}

/// 物理 RAM 上界（不包含），用于恒等映射与帧分配器；QEMU 实现从平台持有的
/// DTB 解析，其它配置返回回退常量。
pub fn physical_ram_end_exclusive() -> usize {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        impl_qemu_riscv64_opensbi::memory::physical_ram_end_exclusive()
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::memory::physical_ram_end_exclusive()
    }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt")))]
    {
        config::mm::QEMU_VIRT_PHYS_RAM_END
    }
}

/// 平台组合层自检：验证引导上下文和物理内存边界已经可查询。
#[cfg(feature = "self_test")]
pub fn self_test() {
    let dtb = dtb_pa();
    let ram_end = physical_ram_end_exclusive();
    assert!(ram_end > 0, "platform RAM boundary must be non-zero");
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::self_test();
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::self_test();
    log::info!("[platform] self_test ok: dtb={:#x} ram_end={:#x}", dtb, ram_end);
}

/// 启动参数与引导上下文：具体类型由 feature 选中的 `platform-impl` 提供。
#[cfg(feature = "api-v0")]
pub mod boot;

/// 架构层再导出：trap、时间计数、中断控制、分页等与 ISA 直接相关的 API。
///
/// 与 **platform-impl** 的边界：仅操作 CPU / 监管态可见的硬件与 CSR，不选择板级后端。
pub mod arch {
    pub use arch::*;

    /// 执行当前 ISA 的极早初始化（例如安装 trap 向量）。
    ///
    /// BOOT_CONTRACT: 必须在打开全局中断之前调用；可重复性由各 arch impl 决定，
    /// 调用方不应把它当成可在运行期反复执行的初始化接口。
    pub fn init() {
        arch_boot();
    }
}

/// Platform SMP lifecycle. Only the RISC-V OpenSBI profile currently starts
/// secondaries; other profiles deliberately report `Unsupported`.
#[cfg(feature = "api-v0")]
pub mod smp;

/// 平台层时间频率：由 `PlatformTime` 实现（通常来自板级 / DTB / 环境常量），
/// **不**等同于 arch 的 `time` CSR 读频率（arch 侧可能返回不支持）。
///
/// 引导期可通过 [`set_frequency_hz`] 注入 DTB 探测结果；未设置时回退
/// `PlatformTimeImpl::get_time_frequency_hz`。
#[cfg(feature = "api-v0")]
pub mod time;

/// 组合定时器：用 arch 的 tick 与 [`crate::time`] 给出的 Hz 换算时刻，再经
/// 平台后端编程下一次定时器中断。
///
/// 错误变体区分 `Arch` / `Platform` / `DeadlineTimer` 三层来源，便于定位是 CSR、
/// 频率配置还是平台定时器后端调用失败。
pub mod timer;
/// 系统复位与关机：由当前 platform impl 提供，不经由 arch 特权指令封装。
pub mod reset {
    pub use crate::active_impl::reset::{reboot, reset, shutdown};
    pub use api_v0::reset::{
        PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
    };
}

/// 早期控制台输出：由当前 board feature 选择 OpenSBI、UART 或其它平台后端。
pub mod console;
/// 墙上时钟（`CLOCK_REALTIME` 偏移 + 单调时钟）。
pub mod wall_clock;

/// 中断控制原语再导出：属于 **arch** 层（如 `sie` / `sstatus`），非 platform impl。
pub mod interrupt {
    pub use arch::interrupt::*;
}
