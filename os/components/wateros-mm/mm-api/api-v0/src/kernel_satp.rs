//! 内核地址空间 token 的快速运行期缓存。
//!
//! `kernel_satp` 是历史命名；缓存内容在 RISC-V 上是 `satp`，在 LoongArch64
//! 上是 PGDL+ASID token。由 MM 实现在 `init` 末尾写入，供 trap 返回路径（task runtime）
//! 在决定返回内核/用户态时读取对应的地址空间 token。
//!
//! 放在本 crate 而非 `wateros-mm` 中，是因为 task crate 依赖 `mm-api` 但不依赖
//! `wateros-mm`（后者通过 `impl-sv39 → vfs → … → task` 存在循环依赖）。

use core::sync::atomic::{AtomicUsize, Ordering};

/// 已发布的内核地址空间 token；0 表示 MM 尚未完成初始化，不能用于切换页表。
static KERNEL_SATP : AtomicUsize = AtomicUsize::new(0);

/// 写入内核地址空间 token（由 MM `init` 末尾调用）。
/// 发布顺序由 MM 初始化和页表切换路径共同保证；本缓存本身不替代必要的 TLB 同步。
#[inline]
pub fn set(token : usize) { KERNEL_SATP.store(token, Ordering::Release); }

/// 读取已缓存的内核地址空间 token。
#[inline]
pub fn get() -> usize { KERNEL_SATP.load(Ordering::Relaxed) }
