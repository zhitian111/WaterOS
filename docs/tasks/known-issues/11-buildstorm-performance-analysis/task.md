# K-53..K-58：BuildStorm wait-hot 采样后任务列表

## 状态与来源

本文件依据
[`docs/tasks/perf/baseline/history/waithot-full-analysis-20260807.md`](../../perf/baseline/history/waithot-full-analysis-20260807.md)
整理。当前干净 K-50 基线完整 BuildStorm 最优为 `elapsed_s=1281.26`，目标 `700-800s`。
本次采样没有改内核，使用 pc-hot + wait-hot QEMU 插件。

结论摘要：

- `cargo xtask` 返回竞态仍阻断完整 Final 的可验收采样。
- `mprotect` 是当前采样中最大的单一符号热点。
- BuildStorm 编译期 CPU 负载明显不均，CPU5 最忙，多个核长期停在
  `__wateros_idle_task_runtime_main`。
- `memcpy/memset/memcmp`、VirtIO/block cache、TLSF 仍是后续候选热点。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/tasks/known-issues/README.md`
- `docs/tasks/history/perf/waithot-full-analysis-20260807.md`
- `docs/tasks/history/perf/pc-hot-analysis-log.md`
- `docs/tools/pc-hot.md`

## 并行关系

K-53 是完整验收的前置阻断，应优先单线处理。K-54 与 K-55 有部分共享风险：
K-54 修改 MM 页表，K-55 修改调度器；两者都可独立实验，但完整验收都要等 K-53。
K-56、K-57、K-58 是测量后候选，可以并行分析，但不能在没有完整基线的情况下直接
合入。

## K-53：修复 `cargo xtask` 返回竞态

### 状态

- [x] 已修复

### 任务目标

BuildStorm 在 `[axbuild] ... done` 后必须稳定返回并打印
`BUILDSTORM_COMPILE mode=multi ok=true`，不能偶发卡在 `cargo xtask` 退出路径。

### 已知信息与代码证据

- 2026-08-07 两次完整采样：一次双插件 28 分钟卡在编译阶段，一次 wait-hot 已完成
  `[axbuild] ... done (1204.05s)` 后 7 分钟仍未打印 `BUILDSTORM_COMPILE`。
- 历史记录显示卡死时 `/work/.build.rc` 不存在，串口已出现 `done`，但 `cargo xtask`
  没有返回。
- `(sleep 10 &) | cat` 不复现；增量 `cargo xtask` 可正常返回；说明问题在完整重编译
  的长负载、多进程、多 fd、exec/exit 组合路径。
- `wateros_debug.py` 40 分钟未抓到 stable 停滞，说明不是简单的固定 PC 死循环。

### 涉及文件

- `os/components/wateros-task/task-scheduler/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/src/user_bringup_common.rs`
- `os/scripts/debug/wateros_debug.py`

### 任务内容

1. 在完整轮 axbuild 结束后，对 `cargo xtask` 主进程和子进程加低频状态/PC/fd 采样，
   确认最终停滞点。
2. 检查 exec、exit_group、wait/reap、管道和 fd 继承组合；优先排除“子进程已退出但父
   进程仍认为它未退出”或“fd 已关闭但 wait 条件未发布”的竞态。
3. 用短复现脚本模拟完整重编译后返回，直到不再依赖 20 分钟全量轮。
4. 修复后先连续三轮 RISC-V Final 通过，再跑 LoongArch Final。

### 如何验收

- [x] RISC-V Final 两轮打印 `BUILDSTORM_COMPILE ok=true`。
- [x] LoongArch Final 两轮通过。
- [x] `make rv_check && make la_check` 通过。
- [x] 不再依赖“重跑一次碰运气”完成验收。

修复计划与根因假设见
[`11a-cargo-xtask-return-race-plan.md`](../11a-cargo-xtask-return-race-plan/task.md)。
结果见
[`results/k53-cargo-xtask-return-race-20260807.md`](../11a-cargo-xtask-return-race-plan/history/k53-cargo-xtask-return-race-20260807.md)。

## K-54：验证并优化 `mprotect` 热路径

### 状态

- [ ] 测量后候选

### 任务目标

确认 `Sv39AddressSpace::mprotect` 82.28B 指令是否为真实主导热点；若是，降低
BuildStorm 总耗时。

### 已知信息与代码证据

`sys_mprotect` 当前路径：

```rust
mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
    MmapOps::mprotect(aspace, VirtAddr(addr), len, perm)
})
```

`mprotect` 会逐页执行：

```rust
let mut vpn = addr.floor_page();
let vpn_end = end.ceil_page();
while vpn.0 < vpn_end.0 {
    if self.translate_addr(vpn.start_addr())?.is_none() { ... }
    if perm_u.writable() && !self.ensure_private_for_write(vpn)? { ... }
    self.protect_page(vpn, perm_u)?;
    vpn = VirtPageNum(vpn.0 + 1);
}
```

### 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`

### 任务内容

1. 先区分 mprotect 调用来源：是 guest runtime 高频调用，还是内核内部错误触发。
2. 对比 Linux：相同参数下不应执行不必要的 `ensure_private_for_write` 或逐页 walk。
3. 若权限未变化、范围已映射且页表权限一致，应直接返回，不做全区间遍历。
4. 若真实高频调用来自用户态，评估是否可减少用户态调用次数或合并范围。

### 当前优化思路

已否决：延迟 COW 会把复制成本转移到逐页写 fault，完整轮反而更慢。

当前采用更保守的方案：

- 逐页先读取当前 PTE 权限；如果与目标权限相同，跳过 COW 和 PTE 写入。
- 只有确实发生权限变化时，才执行 `ensure_private_for_write()` 与
  `protect_page()`。
- 如果整次 mprotect 没有 PTE 变化，不执行全局 TLB flush。

这不会改变 Linux COW 语义，只减少无谓页表 walk 与 TLB 刷新。

2026-08-07 第二版实验：180s 同窗口下 mprotect 仍约 `59.5M`，`handle_cow_fault`
从基线 `114.8M` 略升到 `120.5M`，没有可测收益。已回退；mprotect 真实热点是否
出现在完整轮后期，需要先统计完整轮 mprotect 调用次数、范围与 no-op 比例，再决定
是否继续优化。

2026-08-07 实验结论：之前“mprotect 82.28B”来自 28 分钟完整轮，不能与 180s 早期
窗口直接对比。同一 180s 早期窗口下，干净基线和 K-54 实验的 mprotect 都是约
`59.5M`，没有显著下降；K-54 的 `handle_cow_fault` 从 `114.8M` 升到 `145.9M`。
完整 RISC-V Final `elapsed_s=1410.77`，比基线 `1296.63` 明显更慢。本轮已回退，
不进入性能提交。

### 如何验收

- [ ] 180s pc-hot A/B 中 `mprotect` 指令显著下降。
- [ ] 完整 Final 和 Pre 通过。
- [ ] mmap/mprotect/COW/lazy page fault 定向回归通过。

## K-55：修复 BuildStorm 编译期 CPU 负载不均

### 状态

- [ ] 确认未闭环（wait-hot 数据）

### 任务目标

让 8 个 vCPU 在 BuildStorm 编译期更均匀地承担负载，减少 CPU5 过忙、其它核大量
WFI 空转。

### 已知信息与代码证据

wait-hot 完整编译阶段每核 WFI idle 时间：

| CPU | idle_ms |
|---|---:|
| 0 | 1191600 |
| 1 | 827206 |
| 2 | 1171588 |
| 3 | 1193672 |
| 4 | 1204302 |
| 5 | 629270 |
| 6 | 1098327 |
| 7 | 1182251 |

调度器已有 `ReadyPlacement::LeastLoaded` 和 `pick_ready_cpu`，但大量 wake 路径仍使用
`ReadyPlacement::LastCpu`，任务可能长期停在原 CPU。`complete_context_switch` 使用
`LeastLoaded`，但只覆盖 deferred-ready 路径。

### 涉及文件

- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/wait.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/cpu.rs`

### 任务内容

1. 为每个 CPU 增加可复现的 runqueue 长度统计，确认 WFI 数据与 runqueue 一致。
2. 评估 wake/ready 路径是否应改为 LeastLoaded，而不是无条件 LastCpu。
3. 在 timer tick 或 ready 入队时增加低频率负载均衡，避免频繁迁移。
4. 用 wait-hot 对比优化前后每核 idle 分布和完整耗时。

### 如何验收

- [ ] wait-hot 显示 8 核 idle 时间差异明显缩小。
- [ ] 完整 Final 通过且不劣于当前基线。
- [ ] fork/exec/wait/futex/socket 并发回归通过。

## K-56：降低 `memcpy/memset/memcmp` 热点

### 状态

- [ ] 测量后候选

### 任务目标

先区分用户态与内核态拷贝来源，再减少 BuildStorm 中的总拷贝量或优化热路径。

### 已知信息与代码证据

部分采样中：

- `memcpy` 57.73B
- `memset` 13.66B
- `memcmp` 11.51B

候选来源包括页缓存、VFS 路径解析、syscall 用户缓冲、ELF 装载和用户态 libc。

### 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs`

### 任务内容

1. 按 PC 地址区分用户态/内核态 `memcpy`，并统计调用栈。
2. 检查页缓存和块设备路径是否存在不必要的整页复制。
3. 检查 `user_copy` 是否可减少逐字节/逐页校验与拷贝。
4. 优化必须先有 pc-hot A/B，不能凭猜测替换底层库。

### 如何验收

- [ ] 180s pc-hot A/B 中目标拷贝符号下降。
- [ ] read-family、iozone、完整 Final/Pre 通过。

## K-57：降低 VirtIO/block I/O 热路径

### 状态

- [ ] 测量后候选

### 任务目标

降低 `VirtQueue::add_notify_wait_pop` 与 block cache 的指令占用。

### 已知信息与代码证据

部分采样中 VirtQueue `add_notify_wait_pop` 为 7.99B，block cache `read_blocks` 约
4.0B。block cache 已做过 8 路组相联索引、miss-run 插入等优化，说明剩余热点可能在
VirtIO MMIO 通知、队列操作和请求生命周期。

### 涉及文件

- `os/components/wateros-driver/driver-block/`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/`
- `os/vendor/virtio-drivers`（只读调查）
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/`

### 任务内容

1. 反汇编 `add_notify_wait_pop`，区分是 notify、wait、还是描述符维护占主导。
2. 评估批量提交、延迟通知、event index 和避免每次请求 MMIO 写。
3. 先做短采样 A/B，再跑完整 Final。

### 当前优化思路

`BLOCK_CACHE_CAPACITY_BLOCKS` 当前为 `1024`，对应 512B 块设备缓存仅 512KiB。
BuildStorm 会频繁读取 ext4 元数据与文件数据，512KiB 热集太小，导致大量请求穿透到
VirtIO。第一步先扩到 `16384`（8MiB），再用同窗口 pc-hot 对比
`add_notify_wait_pop` / `read_blocks` 是否下降。

### 如何验收

- [ ] pc-hot A/B 中 VirtIO/block 热点下降。
- [ ] iozone、BuildStorm 和 Pre smoke 通过。

## K-58：降低 TLSF 内核堆热点

### 状态

- [ ] 测量后候选

### 任务目标

降低内核堆 allocate/deallocate/reallocate 的指令与锁竞争。

### 已知信息与代码证据

部分采样中：

- TLSF `with_allocator_interrupt_guard` 7.19B
- TLSF `allocate` 4.46B
- TLSF `deallocate` 3.21B

`InterruptSafeTlsfHeap` 在每次 alloc/dealloc/realloc 中都持有单一 `spin::Mutex`，
并执行 mem_stats 与 estimate 更新；8 核并发分配会共享同一把锁。

### 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`
- 热路径中减少分配：页缓存、VFS 句柄、procfs、fd registry

### 任务内容

1. 统计 BuildStorm 中分配 size 分布和来源。
2. 评估 per-CPU 小对象缓存、减少 allocator 锁粒度，或减少高频短生命周期对象。
3. 防止 128 MiB 堆碎片和页缓存/ramfs 内核堆依赖扩大。

### 如何验收

- [ ] 180s pc-hot A/B 中 TLSF 符号下降。
- [ ] 完整 Final/Pre 通过，内存峰值不劣化。

## 交付规则

每项完成后按 known-issues 约定写入
`docs/tasks/history/known-issues/`，包含 commit、QEMU 参数、pc-hot/wait-hot 数据、
完整/Pre 结果、原始日志路径和 SHA-256。
