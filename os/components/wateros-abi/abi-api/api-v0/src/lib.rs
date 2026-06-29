#![no_std]
//! WaterOS ABI v0：错误码、系统调用参数包、调用号抽象及用户态返回值编码。
//!
//! 语义与 Linux 用户态 libc 约定对齐，便于用户程序与内核之间保持可预期的边界。

pub mod errno;
pub mod syscall_args;
pub mod syscall_number;
pub mod user_ret;
