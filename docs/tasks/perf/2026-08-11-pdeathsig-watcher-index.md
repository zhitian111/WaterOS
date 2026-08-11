# PDEATHSIG 稀疏 watcher 反向索引（2026-08-11）

## 为什么选择这里

BuildStorm 高频执行 fork/clone/exit。当前 `ProcessRegistry` 的父进程死亡通知没有按
父源建立索引：

```text
mark_task_exited(task)
  -> take_parent_death_notifications(Task(task))
      -> 扫描 processes 中的全部进程
  -> 进程最后一个线程退出时
      -> take_parent_death_notifications(Process(pid))
          -> 再扫描全部进程

mark_process_exited(pid)
  -> 对进程内每个 task 分别扫描全部进程
  -> 再为 Process(pid) 扫描全部进程
```

绝大多数进程的 `parent_death_signal` 为 0，却仍在每个线程退出时参与全表扫描。
历史完整测试也显示，引入 subreaper/PDEATHSIG 生命周期语义后，RISC-V BuildStorm
从 1035.07s 上升到 1094.26s/1148.24s；已有 subreaper 与 FIFO/RR 快速路径只恢复到
1116.56s。这不能单独证明全部回退都来自本函数，但说明退出生命周期值得做结构性
优化，而不是继续增加缓存容量或缩短冷门 syscall。

## 选择的优化方案

在 `ProcessRegistry` 中增加：

```text
BTreeMap<ParentDeathSource, BTreeSet<ProcessId>>
```

它只记录 `parent_death_signal > 0` 的进程，因此是稀疏索引：

1. `PR_SET_PDEATHSIG` 从 0 变为非 0 时，验证当前 source 仍存活，再登记 watcher；
   变为 0 时移除 watcher；非 0 攮信号只更新信号值，不重复登记。
2. source 退出时直接 `remove(source)` 取出 watcher 集合，只访问真正需要通知的
   进程；不再扫描 `processes.values_mut()`。
3. orphan 被重挂到 subreaper/init 时，通过统一辅助函数同时迁移活动 watcher。
4. 进程 abort/reap/remove 和 registry clear 时同步清理索引，禁止悬空 pid。
5. 对 signal=0 的进程允许保留未索引的 source；以后启用 PDEATHSIG 时检查 task/
   process 是否仍存活。若 source 已死亡则清为 `None`，保持“不会补发过去的死亡事件”
   的现有语义。

## 为什么这么做

这个方案把常见的“没有任何 PDEATHSIG watcher”退出路径从 `O(process_count)` 降为
一次 `BTreeMap` 查找；有 watcher 时只处理 `O(log source_count + watcher_count)`。
它不为所有父子关系维护第二份重索引，也不改变 `parent_pid`、subreaper 祖先选择或
信号投递时机，因而修改面集中在 `ProcessRegistry` 内部。

使用有序集合而不是 `Vec`，可以让重复设置、重挂和回收保持幂等，并维持测试可重复
的通知顺序。索引只在 process-registry 锁内访问，不新增锁或跨锁依赖。

## 必须保持的不变量

- `parent_death_signal > 0 && parent_death_source == Some(source)` 的活跃进程必须且
  只能出现在 `watchers[source]` 一次。
- signal=0 的进程不得占用 watcher 索引。
- source 死亡后同一 watcher 只通知一次；重挂后可在新 subreaper/init 死亡时再次
  通知，保持现有语义。
- fork 子进程仍以 signal=0 开始，且 source 仍绑定创建它的线程。
- 回收/abort 后索引中不得残留被移除 pid。

## 接下来的优化工作

1. 实现 source 存活检查和 watcher 登记、注销、迁移、取出辅助函数。
2. 替换 `take_parent_death_notifications` 全表扫描，并接入 set、reparent、remove、
   clear 路径。
3. 保留现有 `parent_death_signal_tracks_creating_thread_once`、
   `exit_group_notifies_once_then_subreaper_death_notifies_again` 测试，再增加：
   - signal 0/非 0 切换不会重复登记；
   - source 已退出后才设置非 0 不会迟发；
   - watcher 进程回收后索引不残留。
4. 运行该 crate 定向测试、双架构 check 和 RISC-V Final 构建。
5. 先跑固定窗口 pc-hot 确认退出扫描消失，再在同镜像、同 affinity、同临时目录下
   交错运行至少两轮 main/候选 BuildStorm；只有可重复且至少 1.5% 的收益才合并回
   main，否则回退代码并保留失败文档。

## 当前状态

- 方案已实现，RISC-V/LoongArch check 与 RISC-V Final 构建通过。
- 首批性能样本发现测量环境失控，不能用于验收：根分区使用率 99%，QEMU
  `-snapshot` 临时层落在同一分区；宿主同时包含 0-15 的 P 核逻辑 CPU 和 16-31
  的 E 核，旧 runner 未固定 affinity；此外旧 809.42s 基线与当前镜像 SHA 不同。
- 后续验收统一改为 `TMPDIR=/tmp`、输出根目录 `/tmp`、`taskset -c 0-15`，并在
  完全相同的镜像 SHA 上交错运行 main/候选。此前 `pdeathsig-full-a1=869.47s` 和
  `main-control-full-a1=889.52s` 仅作为发现环境问题的校准样本，不计入收益。

## 验证与性能结果

### 静态与构建验证

- `make rv_check`：通过。
- `make la_check`：通过。
- `make kernel-rv-final`：通过。
- 新增了 signal 0/非 0 切换、source 退出后启用不补发、watcher 回收清索引三个
  `ProcessRegistry` 测试。该 no_std crate 的 host `cargo test` 和 RISC-V
  `cargo check --tests` 分别受缺失架构 paging 实现、bare-metal target 无 `test`
  crate/allocator 限制，仓库当前没有可运行的独立 test harness；测试代码本身随否决
  实现一同回退，未把“仅编译内核”误报为单元测试通过。

### 固定环境 BuildStorm

统一条件：

- 当前镜像 SHA256：`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`；
- main kernel SHA256：`97b7d448018ee8e65085a5d017a5469f3fda1b09f508f1e1319f13b099f39b58`；
- candidate kernel SHA256：`d00a0a5c92bac46d63288223eaa855af15180f6c9aa893524d43ff1f0710a835`；
- `TMPDIR=/tmp`、结果写 `/tmp`、`taskset -c 0-15`、QEMU `-snapshot`；
- main/candidate 按 M/C/M/C 交错运行。

| 类型 | run id | guest elapsed | 结果 |
| --- | --- | ---: | --- |
| main | `fixed-main-full-a1` | 810.71s | passed |
| candidate | `fixed-pdeathsig-full-a1` | 798.85s | passed |
| main | `fixed-main-full-a2` | 815.06s | passed |
| candidate | `fixed-pdeathsig-full-a2` | — | 1200s host timeout，正式编译已完成但缺最终 marker，不计入 |
| candidate | `fixed-pdeathsig-full-a3` | 820.39s | passed |

main 两轮中位数为 **812.89s**，candidate 两个有效样本中位数为 **809.62s**；
候选快 3.27s，即约 **0.40%**。候选两个有效样本相差 21.54s，波动明显大于
测得收益。`fixed-pdeathsig-full-a2` 无 panic/stall，cargo 自报 803.05s 后在 objcopy
阶段耗尽 host timeout，但没有最终 BuildStorm marker，因此不用于性能统计。

### pc-hot

`pdeathsig-pchot-a1` 固定采样 302.46s，按预期 timeout，无 panic/stall。索引实现后：

- `mark_task_exited`：约 145.8K 指令；
- `collect_child_pids`：约 94.5K 指令；
- `mark_process_exited`：约 45.6K 指令；
- `take_parent_death_notifications`：约 38.7K 指令；
- `reparent_orphans`：约 22.4K 指令。

这证明全表通知扫描已经被压低，但整个退出通知路径在当前 BuildStorm 中本来就不是
主要热点；top-100 仍由 memcpy/memset/memcmp、virtio 同步等待、TLSF、路径处理、
页表与用户复制主导。

## 验收结论

**否决，不合并代码。** 稀疏 watcher 索引在算法上把无 watcher 的退出路径从全表
扫描降为有序映射查找，但固定环境的端到端中位数收益只有 0.40%，低于 1.5% 门槛，
且候选样本波动大于收益。本分支只保留实验文档，`process.rs` 恢复到 main；后续不要
继续扩展 PDEATHSIG/父子索引，除非新的热点证据显示退出注册表重新成为主要瓶颈。
