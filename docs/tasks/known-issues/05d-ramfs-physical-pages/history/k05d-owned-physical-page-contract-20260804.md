# K-05D RAII 物理页所有权契约阶段报告

## 目标与结果

为 ramfs payload 迁移提供架构无关、不可复制的物理页所有者。新增
`wateros_mm_frame_alloctor::OwnedPhysPage`：

- `alloc_zeroed()` 从全局帧池分配一帧并在返回前清零；
- `as_bytes()` / `as_bytes_mut()` 只在页所有者借用期内暴露 4 KiB slice；
- `Drop` 恰好调用一次 `frame_dealloc_result()`；
- 不实现 `Clone`，dummy 后端返回 `FrameAllocError::Unsupported`；
- `frame_id()` 只供统计和诊断，不转移所有权。

该契约位于 frame-allocator aggregate，不依赖 Sv39 或 LoongArch 页表实现。它复用两套
内核页表已经建立的“可分配 RAM 完整恒等映射”契约。Cargo 依赖图仍为
`fs -> frame-allocator -> mm-api`，不形成 `fs <-> mm-impl` 环。

## 验证

- `make check`：通过。
- `make la_check`：通过。
- `cargo metadata --no-deps --format-version 1`：通过。
- host `cargo test`：仓库默认 feature 会在 x86_64 编译 `sbi-rt`，因 RISC-V 寄存器
  不可用而失败；不是本次代码错误。真实分配、清零、读写和 Drop 将由后续 ramfs
  kernel self-test 与 QEMU frame 计数验证。

本提交只冻结页所有权边界，尚未改变 ramfs 存储，K-05D 仍未完成。
