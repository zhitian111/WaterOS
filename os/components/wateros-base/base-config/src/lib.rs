#![no_std]
//! 与具体板级解耦的基础配置常量：系统调用参数上限、内核堆尺度与 QEMU `virt`
//! 内存布局假设等。
//!
//! 供 MM、runtime、task、IPC 与 syscall 在 `#![no_std]` 下共享，避免魔法数
//! 重复定义。这里仅保存编译期配置，不放运行时状态和可变策略。
//! 修改任一容量或地址时，必须同步检查使用它的地址区间计算、溢出处理和两种目标架构
//! 的 feature 条件；本 crate 不负责从设备树覆盖这些编译期默认值。

pub mod fs;
pub mod ipc;
pub mod klog;
pub mod mm;
pub mod syscall;
pub mod task;
