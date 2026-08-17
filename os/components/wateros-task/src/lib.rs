#![no_std]

extern crate alloc;
mod cpu;
mod lifecycle;
mod process;
mod runtime;
pub use runtime::register_idle_maintenance_hook;
pub mod sched;
mod schedule;
mod spawn;
mod trap;
pub mod wait_queue;
pub use self::wait_queue::WaitQueue;
pub use api_v0::CpuMask;
pub use lifecycle::*;

pub use process::*;
pub use sched::*;
pub use schedule::*;
pub use spawn::*;
pub use trap::*;
mod scheduler {
    pub use scheduler::*;
}
pub use api_v0::*;
pub use cpu::*;
pub(crate) use impl_core as active_impl;
pub use scheduler::CpuSnapshot;

// ============================================================================
// 与主函数的接口
// ============================================================================
/// 初始化任务系统和底层调度器状态。
pub fn init() {
    scheduler::init();
    active_impl::init_process_registry();
}

#[cfg(feature = "self_test")]
/// 任务组件内核态自检；只验证调度器已建立，不创建或切换用户任务。
pub fn self_test() {
    log::info!("[task] self_test begin");
    active_impl::self_test();
    let mask = online_cpu_mask();
    assert!(mask.bits() != 0, "at least the boot CPU must be online");
    let idle_ticks = total_idle_ticks();
    log::info!("[task] self_test observed idle_ticks={}", idle_ticks);
    log::info!("[task] self_test complete; no task state was mutated");
}
/// 启动调度器并切入第一批可运行任务。
pub fn run_first_task() -> ! { scheduler::run_first_task() }
