# allocator interrupt guard 快速路径方案（2026-08-11）

## 为什么选择这里

TLSF 诊断显示：

```text
tlsf_lock_acquire=31094581
tlsf_lock_contended=718213
```

锁竞争率只有约 2.3%，说明 allocator 主要成本不是跨核锁等待，而是每次
alloc/dealloc/realloc 的固定包装开销。pc-hot 中：

- `with_allocator_interrupt_guard` alloc 路径：645,268,571 条指令
- dealloc 路径：496,344,531 条指令
- realloc 路径：103,160,031 条指令

当前 guard 每次都会读取中断状态、执行 `disable_global_interrupt()`，退出时又执行
`restore_global_interrupt_state()`。内核中大量分配路径进入时中断本来就已关闭，
这些重复 CSR 操作不改变语义，只增加固定成本。

## 选择的方案

给 `ArchInterruptState` 增加 `global_interrupts_enabled()`，在 guard 中只做：

```rust
let state = read_global_interrupt_state();
let was_enabled = state.global_interrupts_enabled();
if was_enabled {
    disable_global_interrupt();
}
let depth = ...;
...
if was_enabled {
    restore_global_interrupt_state(state);
}
```

- 如果进入前中断已关闭，不重复 disable，退出时也不 restore，保持原状态。
- 递归分配检测仍通过 per-CPU depth 完成，不允许嵌套。
- RISC-V 使用 `sstatus.SIE` bit 1；LoongArch 使用 `CRMD.IE` bit 2。

## 为什么这么做

1. 不改变 TLSF 算法、锁粒度或递归语义。
2. 只是跳过原本无效果的 CSR 写，减少固定路径指令。
3. 双架构对称，不需要改 allocator 后端。

## 接下来的工作

1. 在 `perf/allocator-guard-fastpath` 分支实现 API 方法并修改 guard。
2. 双架构 `make check`。
3. 180 秒 smoke 确认无中断状态回归。
4. 完整 RISC-V BuildStorm A/B；有效则合并 main，无效则回退并记录。

## 验收标准

- 双架构 Final check 通过。
- 普通 Final 无 panic、无中断永久关闭、无递归分配告警。
- 完整 BuildStorm 相对 `880.44s` 有可复现改善。

## 实测结果（2026-08-11）

```text
guard-fastpath-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=867.22
guard-fastpath-full-a2: BUILDSTORM_COMPILE ok=true elapsed_s=868.62
main-5a080c07-full-b1:  BUILDSTORM_COMPILE ok=true elapsed_s=874.46
```

两轮 guard 中位/均值约 `867.9s`。同一宿主上 main 复测为 `874.46s`，比历史
`880.44s` 快约 6s，说明存在约 0.7% 的宿主/镜像冷热噪声。guard 相对本轮 main
约快 0.8%，未达到 1.5% 合并门槛。

下一步改为在 guard 快路径之上继续测试“进入 guard 后中断已关闭，因此 depth 只需
普通 per-CPU load/store”，两者作为组合候选重新 A/B；若组合仍不足 1.5%，仅保留
guard 实验分支与文档，不回退 main。
