# K-06A 堆分配器中断保护修复结果（2026-08-03）

## 结论

修复 SMP 压力运行中偶发的 `recursive heap allocation detected` panic。根因不是分配器
backend 缺少跨核互斥，而是每 CPU 递归深度和中断状态的更新顺序留下了抢占窗口。

## 修改

- 先读取并关闭本核中断，再增加 allocator 递归深度；退出时先清除深度，再恢复中断。
- 递归检测 panic 前恢复深度和原中断状态。
- linked-list 与 TLSF backend 的高水位日志移到 allocator guard 外，避免日志路径在
  guard 内再次分配。

涉及文件：

- `os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_linked_list.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`

## 验证

- `make check`：通过。
- TLSF feature 的 RISC-V `cargo check`：通过。
- 新主办方 RISC-V 镜像、OpenSBI、8 CPU、8 GiB 的 final 连续运行超过两小时，越过
  原约 30 分钟 panic 点，未再次出现 allocator 递归 panic。
- 本轮最终停在 BuildStorm `rustc` 的 futex/线程退出阶段，与 allocator 无关。
- 停机后对镜像执行 `e2fsck -fn`：无结构损坏，仅有 extent tree 可优化提示。
