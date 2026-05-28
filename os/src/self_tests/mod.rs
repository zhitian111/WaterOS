//! 仅在 `feature = "qemu-riscv64-opensbi"` 下编译的内核态自检子树。
//!
//! **用途**：在真实 bring-up（`kernel_main`）完成
//! MM、驱动与（可选）文件系统栈之后，
//! 追加调度、等待队列、回收等方面的回归验证（无固定根卷 ELF 用户任务）。
//!
//! **入口**：任务相关自检的统一入口为 [`task::spawn_all`]；模块级说明与各 stage
//! 语义见 [`task`]。
pub mod network;
pub mod task;
