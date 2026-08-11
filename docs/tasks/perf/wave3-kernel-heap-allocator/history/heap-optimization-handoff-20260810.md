# WaterOS 堆优化交接（2026-08-10）

## 目标与约束

从已验证的 `main` 基线继续优化 BuildStorm，优先处理堆分配器及其调用方。用户要求按以下方向依次推进：

1. 消除高频调用方分配（优先于缓存所有小对象）。
2. 只为统计确认的单一热点 size class 试验 per-CPU cache，不上完整八档 slab。
3. 优化 allocator interrupt guard：识别进入前中断已经关闭的情况，避免无效 disable/restore；真正递归分配仍必须报错。
4. 对 TLSF 高频调用点做对象复用、栈缓冲或固定容量缓存。
5. 测量真实锁竞争，不用 TLSF 指令占比代替锁等待。

性能功能只有相对 `main` 的完整 BuildStorm 净改善达到 1.5% 才能合并；诊断/正确性基础设施退化不得超过 2%。每个实验应独立验证，失败实验不要叠加进候选。

## Git 与工作区现状

- 当前分支：`perf/heap-main-optimization`
- 当前 HEAD：`5a080c078a4e4b894cd9308cd709e4359d729b82`
- HEAD 提交：`[perf] share cached ext4 inode snapshots`
- `main` 也指向上述提交；已验证基线为 `880.44s`。
- 分支是直接从 `main` 创建的，没有带入旧 slab 或文件页代码。
- 当前有未提交的 TLSF 诊断草稿，见下文。不要在不了解其问题的情况下提交。
- `user` 子模块原本就是 dirty，必须保持不动。
- 仓库中有许多既有未跟踪资料/内核/测试镜像，均属于用户，不要清理或纳入提交。
- 交接时已经确认没有存活的 `buildstorm_runner.py` 或 QEMU 进程。

相关历史分支只作参考，暂时不要合并：

- `perf/tlsf-slab`：完整八档 slab，BuildStorm `910.08s`，相对 main 退化 3.37%，不可合并。
- `feat/file-page-sharing`：文件页工作独立保存。
- `wip/tlsf-filemap-infrastructure`：旧组合归档。

## 当前未提交改动

以下文件构成一个“最小 TLSF 诊断草稿”：

- `os/Cargo.toml`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/tlsf_diagnostics.rs`（未跟踪新文件）
- `os/src/user_bringup_common.rs`

草稿增加独立 `tlsf-diagnostics` feature，统计九个 size bucket、free/realloc、alignment、OOM，以及用 `try_lock` 观察锁竞争。普通 Final 通过 `cfg` 完全排除计数。

静态检查已通过：

```bash
cd os
make check ARCH=rv PROFILE=final EXTRA_FEATURES=tlsf-diagnostics
make check ARCH=la PROFILE=final EXTRA_FEATURES=tlsf-diagnostics
git diff --check
```

但运行诊断失败：计数使用全局 `AtomicU64::fetch_add`，开销过大。300 秒内连 toolchain/minibuild marker 都没到，`perf_counters` 为空。因此必须先重构成真正 per-CPU、本地中断已关闭时的普通 load/store（或明显降采样），再跑诊断；不要使用这轮空结果选择 size class。

失败诊断记录：

- 目录：`os/tem/perf/buildstorm/main-tlsf-diagnostics-300/`
- 结果：`result.json`
- 串口：`serial.log`
- 内核：`os/tem/perf/buildstorm/kernels/kernel-rv-main-tlsf-diagnostics`
- 内核 SHA256：`ae00b5cc08cab830b90d309d147edccae1c2e15f2966e23116924bfae01678f0`
- 状态：固定 300 秒 timeout；无 panic/SIGSEGV/stall，但 required markers 全为 false。

建议保留 feature 和输出接口，但把计数存储改为 `CpuLocal` 槽。allocator guard 已关闭本地中断，因此每 CPU 槽可以用普通整数或原子 load/store，避免 locked RMW。输出阶段再汇总所有 CPU。若实现复杂，可先按 1/64 或 1/256 采样，并在结果中记录采样率。

## 已完成的纯 main pc-hot

已经重新构建 exact-main RISC-V Final 内核并采集干净的 300 秒 pc-hot：

- 内核：`os/tem/perf/buildstorm/kernels/kernel-rv-main-5a080c07-final`
- SHA256：`087fb82e788b479f3f0bce4230f7040233c0c4cc8c1d13024f835389e0070866`
- 运行目录：`os/tem/perf/buildstorm/main-5a080c07-pchot-300/`
- 原始 PC：`pc-hot.txt`
- 符号分析：`pc-hot-analysis.txt`
- 元数据：`result.json`
- 诊断按预期 300 秒 timeout；无 panic、SIGSEGV 或 stall；toolchain/minibuild marker 已出现。

主要热点（采样指令数）：

- compiler-builtins memcpy：`3,927,959,866`
- memset：`1,919,823,727`
- VirtQueue `add_notify_wait_pop`：`1,653,843,192`
- memcmp：`1,633,779,522`
- TLSF allocate：`1,170,997,363`
- TLSF deallocate：`828,466,979`
- allocator guard alloc 路径：`645,268,571`
- `normalize_absolute_path`：`613,769,407`
- `copy_from_user`：`530,370,833`
- allocator guard dealloc 路径：`496,344,531`
- `ProcessRegistry::process_task_snapshot`：`211,940,139`
- mount namespace snapshot：`119,698,038`
- `String::clone`：`53,203,824`

重要解释：pc-hot 没有调用栈，不能仅凭 TLSF 符号确定具体调用方，也不能给出 size class。`process_task_snapshot` 和 scheduler `task_snapshot` 当前返回固定大小快照，本身不是堆分配；不要误把它们当成分配源。

## 已知历史实验，避免重复

- 完整 slab：`910.08s`，失败。
- page-cache lightweight key：`903.72s` 对当时 `900.64s`，退化 0.34%，已回退。
- mmap 页表递归 gap scan：`905.58s` 对 `900.64s`，退化 0.55% 且尾部 stall，已回退。
- VFS route 锁内借用 + String transfer：`904.59s` 对 `880.44s`，退化 2.74%，已回退。
- 仅 String ownership transfer：`883.96s` 对 `880.44s`，退化 0.40%，已回退。
- user path 整页/128B copy：`898.65s` / `901.04s`，均退化超过 2%，已回退。

因此虽然 `mount_namespace_snapshot`、`normalize_absolute_path` 和 user path 很热，不应机械重做上述方案。若再次触碰，必须提出结构上不同且可解释的新方案。

## 下一步建议顺序

### 1. 修复诊断开销并重新采集

先把 `tlsf_diagnostics.rs` 改为 per-CPU 计数或低频采样。至少保留：

- alloc/free/realloc 九个 bucket：`16/32/64/128/256/512/1024/2048/>2048`
- alignment `>16`
- lock acquire 与 `try_lock` miss
- OOM

诊断内核运行前检查并终止遗留进程：

```bash
ps -eo pid,ppid,stat,%cpu,etime,args | rg '[q]emu-system-(riscv64|loongarch64)|[b]uildstorm_runner.py' || true
```

旧 runner 只存在于 `perf/tlsf-slab`。本次用临时 worktree `/tmp/wateros-perf-tools.VTBMwL` 运行；该目录可能在重启后消失。更稳妥的方式是重新创建临时 worktree，不要把旧 runner/slab cherry-pick 到优化分支：

```bash
TOOL_TREE=$(mktemp -d /tmp/wateros-perf-tools.XXXXXX)
git worktree add --detach "$TOOL_TREE" perf/tlsf-slab
ln -s /home/zhitian/project/WaterOS_refactor/final_test_case "$TOOL_TREE/final_test_case"
```

runner 位于 `$TOOL_TREE/os/scripts/perf/buildstorm_runner.py`。输出必须指回主仓库的 `os/tem/perf/buildstorm`。

先做 300 秒诊断；若仍然明显慢于纯 main 进度超过 2%，继续降低统计频率。诊断通过后提交为独立 diagnostics commit。

### 2. 找第一个真正的调用方分配

size class 只能告诉“多大”，还要定位“谁分配”。优先组合以下证据：

- `pc-hot-analysis.txt` 中显式出现的 `RawVec::finish_grow`、`String::clone`、`alloc::fmt`、mount snapshot、page cache install 等符号。
- CodeGraph 查询上述符号的真实调用链与所有权流。
- 必要时为少数候选调用点增加 feature-gated 计数，不要在 global allocator 中做昂贵回溯。

首选目标应是生命周期清晰、可以消除分配而非转移分配的调用点，例如：

- 重复构造的固定小 `Vec`/`String` 改为调用方持有复用缓冲；
- 小且有明确上限的数据改为栈数组或固定容量容器；
- 只查询少数字段时增加轻量 query API，避免构造带 `Vec/String` 的完整快照。

每个候选先两架构 check，再做完整 BuildStorm。未达到 1.5% 不进入 main 候选。

### 3. 单一 size class cache

只有诊断显示某一档占比明显、生命周期适合且调用方消除分配不足时才实现。不要复制旧八档 slab。建议：

- 只支持一个编译期固定 class。
- per-CPU 小容量 freelist/magazine，无额外分配。
- 其余尺寸全部直达 TLSF。
- 先验证正确性与 OOM drain，再跑完整 A/B/B/A。
- 若没有至少 1.5% 净改善，删除/默认关闭该优化，只保留实验记录。

### 4. allocator interrupt guard

当前实现文件：`os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`。

现状：每次都 `read_global_interrupt_state`、`disable_global_interrupt`、深度原子 RMW、最后 restore。旧 slab 分支只把 depth 的 `fetch_add/sub` 改成了 load/store，但没有跳过本来已关闭中断时的 disable/restore。

安全语义：

- 真正 `GlobalAlloc` 递归必须继续 panic，不能因“中断已关闭”而允许嵌套。
- 若进入前中断已关闭，可以避免重复 disable；退出时也不能错误开启中断。
- guard 必须具备 panic/unwind 不适用场景下的清晰恢复顺序。
- 修改后双架构 check，并检查普通 Final 的反汇编/符号，确认快路径确实减少指令。

### 5. 锁竞争结论

只有修复后的 `tlsf_lock_contended / tlsf_lock_acquire` 数据才能决定是否值得做 per-CPU cache。若竞争率很低，TLSF 热点主要是算法与调用次数，应继续消除调用方分配；不要以“TLSF 占 10%+ 指令”为由直接引入复杂 cache。

## 必读文件与工具

开始工作前按顺序阅读：

1. 根目录 `AGENTS.md`（用户提供的 CodeGraph 与仓库规则）。
2. `os/AGENTS.md`，然后完整阅读它要求的 `os/AGENT.md`。
3. 本交接文档。
4. `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
5. `os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`
6. `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
7. 当前草稿 `tlsf_diagnostics.rs`
8. `os/tem/perf/buildstorm/main-5a080c07-pchot-300/pc-hot-analysis.txt`
9. `docs/tasks/known-issues/11-buildstorm-performance-analysis.md`（历史实验记录，未跟踪用户文件，只读）。

仓库根存在 `.codegraph/`，理解或定位代码时必须先用：

```bash
codegraph explore "具体符号、调用链或问题"
```

再用 `rg` 补充文本检索。pc-hot 工具在 `os/scripts/pc-hot/`。性能 runner 在旧 `perf/tlsf-slab` 分支的 `os/scripts/perf/`，建议只通过临时 worktree 使用。

## 验证门禁

每个可保留改动至少执行：

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=pre
make check ARCH=la PROFILE=final
make all
cmp kernel-rv kernel-rv-final
cmp kernel-la kernel-la-final
git diff --check
```

性能轮使用冷镜像、`-snapshot`、固定 runner 和 `A/B/B/A`。启动前必须检查旧 QEMU；出现 SIGSEGV、低 CPU 卡死或长时间无串口输出时先终止并诊断，不要在旧进程未清理时启动下一轮。

普通 Final 必须不含 TLSF histogram/锁等待计数符号。完整 BuildStorm 相对 `880.44s` 改善至少 1.5% 才可合并 main。

## 交接时最终状态

- 无运行中的 QEMU/runner。
- 当前分支正确，HEAD 与 main 相同。
- pure-main pc-hot 已完成且结果完整。
- TLSF 诊断草稿静态检查通过，但运行开销失败，尚未提交。
- 下一位 agent 应先修诊断存储/采样开销，再运行 300 秒计数；不要立即实现 slab。
