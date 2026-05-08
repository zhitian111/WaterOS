//! 帧分配器桩：不导出 `init_frame_allocator` / `frame_alloc_result` 等符号。
//!
//! 供仅编译 mm API、或组合 dummy mm-impl 的 cfg 使用；任何依赖真实物理帧的路径须在 feature 中启用 `impl-stack`。

#![no_std]
