#![no_std]

//! OpenSBI / RISC-V SBI 基础扩展的**固件实现**（控制台、`set_timer`、`system_reset`）。
//!
//! 依赖 `sbi` crate 的封装；与 `arch` 层边界为：**此处不读写 trap 帧或 `satp`**，
//! 仅发出 SBI 调用。

pub mod console;
pub mod reset;
pub mod timer;
