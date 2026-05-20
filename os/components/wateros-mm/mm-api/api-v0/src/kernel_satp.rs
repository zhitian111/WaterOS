//! 内核 `satp`/地址空间 token 的快速运行期缓存。
//!
//! 由 MM 实现（如 `impl-sv39`）在 `init` 末尾写入，供 trap 返回路径（task
//! runtime） 在决定返回内核/用户态时读取对应的地址空间 token。
//!
//! 放在本 crate 而非 `wateros-mm` 中，是因为 task crate 依赖 `mm-api` 但不依赖
//! `wateros-mm`（后者通过 `impl-sv39 → vfs → … → task` 存在循环依赖）。

use core::sync::atomic::{AtomicUsize, Ordering};

static KERNEL_SATP : AtomicUsize = AtomicUsize::new(0);

/// 写入内核地址空间 token（由 MM `init` 末尾调用）。
#[inline]
pub fn set(token : usize) { KERNEL_SATP.store(token, Ordering::Release); }

/// 读取已缓存的内核地址空间 token。
#[inline]
pub fn get() -> usize { KERNEL_SATP.load(Ordering::Relaxed) }
