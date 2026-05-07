//! **内核 trap 路由入口**（打破 `arch-impl` ↔ `task`/`syscall` 的 Cargo 环）。
//!
//! ## 契约
//!
//! - 各 ISA 的 trap 帧布局留在 `arch-impl-*`；本模块只接受 **不透明** `*mut u8`
//!   帧指针，由 **组合层**（如 `wateros`）在 [`KernelTrapHandlerFn`] 内自行 downcast。
//! - **唯一** 的链接可见入口：[`wateros_kernel_trap_enter`]（`extern "C"`），实现仅为「调用已注册的
//!   [`KernelTrapHandlerFn`]」；`arch-impl` 侧应调用 [`invoke_kernel_trap_handler`] 或该符号，**禁止**
//!   再在 `impl` 中写匿名 `extern "C"` 拉取 `task`/`syscall` 等符号。
//! - 组合层须在首次 trap 前调用 [`register_kernel_trap_handler`]（通常紧接 `task::init()`）。

use core::sync::atomic::{AtomicPtr, Ordering};

/// 组合层实现的 trap 处理函数：参数为 **原始** trap 帧字节指针（与各 `trap_entry_*` 传入一致）。
pub type KernelTrapHandlerFn = extern "C" fn(*mut u8);

static HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// 注册组合层 trap 路由；仅保留最后一次注册。
#[inline]
pub fn register_kernel_trap_handler(handler: KernelTrapHandlerFn) {
    HANDLER.store(handler as *mut (), Ordering::Release);
}

#[inline]
fn handler_fn() -> KernelTrapHandlerFn {
    let p = HANDLER.load(Ordering::Acquire);
    if p.is_null() {
        panic!("kernel_trap: register_kernel_trap_handler was not called before trap");
    }
    unsafe { core::mem::transmute::<*mut (), KernelTrapHandlerFn>(p) }
}

/// `arch-impl` 的 `trap_entry_rust` 应调用此函数（Rust 路径），与 [`wateros_kernel_trap_enter`] 等价。
#[inline]
pub fn invoke_kernel_trap_handler(frame: *mut u8) {
    handler_fn()(frame);
}

/// 稳定 **C ABI** 入口，供需要按符号链接的桩/工具链使用；语义同 [`invoke_kernel_trap_handler`]。
#[unsafe(no_mangle)]
pub extern "C" fn wateros_kernel_trap_enter(frame: *mut u8) {
    invoke_kernel_trap_handler(frame);
}
