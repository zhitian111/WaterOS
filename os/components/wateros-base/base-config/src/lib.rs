#![no_std]
//! 与具体板级解耦的基础配置常量：系统调用参数上限、内核堆尺度与 QEMU `virt` 内存布局假设等。
//!
//! 供 ABI、MM 与平台 bring-up 在 `#![no_std]` 下共享，避免魔法数重复定义。

pub mod syscall;

pub mod mm;

pub mod ipc;

pub mod fs;
