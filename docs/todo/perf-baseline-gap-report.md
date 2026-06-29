# 性能优化报告 v2 —— 以「超过 baseline 多拿分」为目标的得分缺口分析

> 数据来源：`/home/zhitian/Downloads/score.txt`、`Riscv输出(3).txt`、`LoongArch输出(3).txt`
> 代码基线：当前 main（已合入第 1 层低风险优化、epoll 等）
> 评测平台：4 套配置 `glibc-la / glibc-rv / musl-la / musl-rv`
> 本报告聚焦「为什么大量样例只到 baseline（score=1.0）而拿不到额外分」，并给出可落地、按收益排序的改法。第 1 层热点目录见同目录 `README.md` 及各 `perf-*.md`，本报告不重复其已实施项。

---

## 0. 先搞清楚评分规则（这是制定策略的前提）

从 `score.txt` 的 `result / baseline / score` 三列反推，性能类测试的计分规则为：

```
score = max(1.0, 实测相对 baseline 的优于程度)        # 测试成功完成
score = 0.0                                            # 测试失败 / 超时 / 数值非法
```

- **吞吐类**（iozone、iperf、netperf，越大越好）：`score = max(1.0, result / baseline)`
  - 例：`iozone fwrite 4 fwriters` LA result=5881.74，baseline=3204.25 → `5881/3204 = 1.455`（行 231）
  - 例：同项 RV result=1728.57，baseline=3204.25 → 比值 0.54，但被**封底为 1.0**（行 231）
- **延迟类**（lmbench、cyclictest，越小越好）：`score = max(1.0, baseline / result)`
  - 例：`Pagefaults` baseline=978.2us，RV-glibc score=1.78 → 实测 ≈ 549us（优于 baseline）
- **功能类**（basic、busybox、libctest、ltp、lua）：`score = 通过用例数`

### 由此得到的核心结论

> **只把性能做到「接近 baseline」一分都不会多拿——必须严格优于 baseline（吞吐更高 / 延迟更低）才有增量。当前绝大多数性能项 score=1.0，意味着我们正好卡在 baseline 之下或附近。**

因此优化目标必须明确为「**翻过 baseline 那条线**」，而不是「比现在快一点」。

---

## 1. 全局得分缺口地图（按可恢复分值从大到小）

| 缺口 | 现象（score.txt 行） | 性质 | 可恢复分值（粗估） | 优先级 |
|---|---|---|---|---|
| **G1. LA-musl LTP 整套 = 0** | 行 1058：musl-la ltp 总分 0，而 musl-rv 568 | **功能性回归（非性能）** | **≈ 568 分（全系统最大单缺口）** | P0 |
| **G2. 块缓存 RV/LA 均未启用 + 读路径放大** | iozone RV 全 1.0（行 230-274）；LA 读全 1.0 | 性能 | iozone 现 84.9，读项翻线后有望 +20~40 | P0 |
| **G3. context switch 计 0** | LA-glibc ctx 8 项全 0（行 610-617）；musl-rv ctx 4 项 0（行 654-657） | 性能（失败=0） | ≈ 10~14 分 | P0 |
| **G4. libc-bench regex_search = 0** | 全 4 配置 b_regex_search=0（行 315/316/348）| 功能/性能 | ≈ 4~6 分 | P1 |
| **G5. lmbench musl-rv Pagefaults = 0** | 行 632 score 0 | 性能（失败=0） | ≈ 1~2 分 | P1 |
| **G6. iozone 写未过线（尤其 RV）** | RV 写 1100~1800 vs baseline 3200~3700（1.0）| 性能 | RV 写约需 2x；可 +8~16 分 | P1 |
| **G7. lmbench 延迟项卡 1.0** | syscall/read/write/stat/open/close/pipe/select/fs-latency 全 1.0 | 性能 | 每项过线 +1~2，潜在 +20~30 分 | P1/P2 |
| **G8. 网络全部卡 1.0** | iperf 6 项、netperf 4/5 项 = 1.0 | 性能 | 翻线 +10~20 分 | P2 |
| **G9. busybox kill/mv/rmdir = 0** | 行 121/126/133（4 配置）| 功能 | ≈ 6 分 | P2 |

> 注意：**G1 是整个系统最大的失分点（约 568 分），且属于功能性问题而非性能**。它使 `musl-la` 总分仅 487，而 `musl-rv` 高达 1040。如果优先级允许，修复 LA-musl 的 LTP 启动/运行（很可能是 bringup 链路或某个早期 panic 导致整套未跑）的收益，**远超所有性能优化之和**。建议先确认它是「未运行」还是「跑了但全挂」。

---

## 2. P0 详解

### G1. LoongArch + musl 下 LTP 整套 0 分（最高优先级，功能性）

- **现象**：`ltp-musl` 表中 `la` 列全部为 `-`（行 868 起），总分 0（行 1058）；而 `glibc-la` 的 LTP 正常（563 分，行 863）、`musl-rv` 正常（568）。即**只有 (LA × musl) 这一组合**完全没有有效成绩。
- **判断方向**：这不是单个 syscall 慢，而是该配置下**测试根本没跑出来**——典型原因：
  1. musl 动态/静态链接器在 LA 上加载失败（execve/ELF 解释器路径）；
  2. LA-musl bringup 脚本未生成或早期 panic；
  3. 该配置镜像缺失 / 挂载失败导致 harness 直接退出。
- **下一步（需先定位再修）**：查 `LoongArch输出(3).txt` 中 musl 段是否有 `panic` / `Segmentation` / `exec format error` / 挂载失败；对比 `os/src/user_bringup_busybox.rs` 与 LA bringup 脚本对 musl 的处理。
- **收益/风险**：收益 ≈ **568 分**；风险取决于根因，可能是定位修复（中）而非架构改造。
- **本报告定位**：性能优化无法弥补此缺口，**强烈建议作为独立最高优先任务并行推进**。

### G2. 块缓存未启用 + 文件读路径放大（iozone 主因）

这是性能侧**收益最高、风险最低**的一组改动。

#### 2.1 块缓存在两个架构的实际构建里都没开（已核实）

- RV：`os/Cargo.toml` 的 `qemu-riscv64-opensbi` feature（行 71-92）**未包含** `driver/impl-block-cache`，故 `impl-qemu-riscv64-opensbi/src/lib.rs:268-275` 的 `#[cfg(feature="block-cache")]` 分支不生效，走裸 VirtIO。
- LA：`driver/Cargo.toml` 的 `impl-block-cache`（行 54-57）**只接 RV**（`impl-qemu-riscv64-opensbi/block-cache`），LA 的 `impl-qemu-loongarch64-virt/src/lib.rs` 内**完全没有** `BlockCacheManager::wrap` 调用（grep 无匹配）。
- 结果：两架构的 ext4 元数据/数据块读都直达 VirtIO，512B 一次往返。这与 **RV iozone 全部读写卡 1.0**、**LA 读卡 1.0** 高度吻合。

**改法（低风险，高收益）**：
1. RV：在 `os/Cargo.toml` 的 `qemu-riscv64-opensbi` feature 列表加入 `"driver/impl-block-cache"`。
2. LA：为 `impl-qemu-loongarch64-virt` 增加 `block-cache` feature 并在 probe 处 `BlockCacheManager::wrap`（对齐 RV 的 `lib.rs:268-275`）；在 `driver/Cargo.toml` 的 `impl-block-cache` 里追加 LA 接线。
3. 扩容：`base-config/src/fs.rs:34` 的 `BLOCK_CACHE_CAPACITY_BLOCKS` 从 **64（=32KiB）** 提到 **256~1024（128~512KiB）**，否则对 MB 级 iozone 文件命中率仍极低。
4. 写策略：当前块缓存写穿且**不 write-allocate**（`impl-block-cache/src/lib.rs:210-227`），write-after 读冷块必 miss；改为写入 LBA 也入缓存，可同时帮到 iozone 的 rewriter/re-reader 项。
- **预期**：LA 读、RV 读写有望从 1.0 翻线；iozone 总分（现 84.9）是最大单块可提升项。
- **验证**：`virtio_blk_probe_test` + 两架构 iozone 对比 + LTP 文件用例回归。

#### 2.2 ext4 读路径每次 IO 重复全路径解析（无 dcache）

- **位置**：`fs-impl/impl-ext4/src/rw.rs:659-677`（`read_range` 每次做 `metadata()` + 两次 `path_to_inode`）；`metadata` 在 `:618-625` 再走一次。复杂度 **O(目录深度 × 目录项数)/次 IO**，与读字节数无关。
- **影响**：iozone 顺序读每页 miss + 预取 8 页都重复解析；也是 **lmbench Simple stat 425us / open-close 501us** 的主因。
- **改法**：① `PagedFileHandle` 绑定 `(inode_no, mount_gen)`，新增 inode 级 `read_range` 绕过 path；② VFS 层 `(mount_gen, path)→inode` LRU dcache，rename/unlink/mount 失效；③ 删 `read_range` 入口的重复 `metadata` 与第二次 `path_to_inode`。
- **风险**：中（缓存失效正确性）；**收益**：iozone 读 + lmbench stat/open/close 多项可翻线。

#### 2.3 读取以 512B 分片 + 页缓存 LRU 为 O(n)

- **512B 分片**：`rw.rs:681-691` `chunk = room.min(BLOCK_SIZE=512)`，一个 4KiB 页 miss 触发 **8 次** ext4 块读；应改 `min(room, FILE_PAGE_SIZE)` 或 ext4 block_size。
- **O(n) LRU**：`vfs-impl/impl-page-cache/src/lib.rs:84-93` `touch_lru` 对 capacity=4096 的 `VecDeque` 线性扫描；读 64KiB ≈ 16 页 → 数万次比较/次。解释 **re-readers（已 warm）仍卡 1.0**。改 O(1) 槽位+侵入式双链表。
- **install_page**：`lib.rs:423` 每 miss `vec![0;4096]` 堆分配 + 驱逐 `clone()`；预取（`:577-585`）**同步**做 8 次完整 `read_range`。改为直接用槽缓冲读 + 批量/异步预取，随机读时关预取。
- **风险**：低~中；**收益**：iozone 顺序读 / re-read 中~高。

### G3. lmbench context switch 计 0（失败丢分）

- **现象**：`lmbench-glibc` 的 `la` 列 ctx switch 2/4/8/16/24/32/64/96 **全 0**（行 610-617）；`lmbench-musl` 的 `rv` 列 ctx 4/8/32 为 0（行 654-657）。而能跑出的样本里 64/96 进程项 score≈1.99（行 615/617），说明大并发下反而「优于 baseline」，**0 分是「没产出有效值」而非「太慢」**。
- **根因方向**（结合代码）：
  1. **fork N 进程 setup 超时/失败**：`fork_cow` 整棵页表树复制（`mm-impl/impl-sv39/src/pagetable.rs:624-658,842-896`）+ 关中断窗口覆盖全部 fd/signal 复制（`sys/clone.rs:239-336`），N 大时 setup 极慢；LA 全局 `invtlb` 更贵 + 无 ASID，更易整体超时（解释 LA-glibc 全 0）。
  2. **定时器抢占污染测量**：`base-config/src/task.rs` 时间片 ≈100ms（10ms tick × 10），ctx 微基准窗口内一旦被 tick 抢占，单样本飙到 ms 级 → 可能被判非法/0。
  3. **stale ready 队列**：`scheduler-impl/impl-round-robin/src/queues.rs:70-83` detach 只 bump version、stale entry 留队，pick_next O(队列长)；大量短命任务时膨胀。
- **改法**：① benchmark/空闲期跳过 promote 或临时拉长时间片（低风险，治 0 分最直接）；② ready 队列 enqueue 时 lazy compact / 侵入式链表；③ 中期：fork 页表结构 COW（高风险，见 G6/perf-memory）。
- **收益**：把 0 分项救回为有效值即 +1~2/项，LA-glibc 8 项 + musl-rv 数项，合计约 10~14 分；风险：①低中、②低中。

---

## 3. P1 详解

### G4. libc-bench regex_search = 0（全配置）

- **现象**：`b_regex_search ("(a|b|c)*d*b")` 与 `("a{25}b")` 在 4 配置全 0（行 315/316/348）。`regex_compile` 正常，仅 **search** 挂——典型为 regex 执行时崩溃/超时/栈溢出。
- **下一步**：查输出日志该用例段是否 panic/超时；可能与回溯型匹配的递归深度或某 syscall 行为相关。
- **收益**：≈ 4~6 分；性质偏功能修复，需先定位。

### G5. lmbench musl-rv Pagefaults = 0

- 行 632：musl-rv `Pagefaults` score 0，而其余三配置正常（rv-glibc 1.78、la 1.99/2.0）。疑为该配置下 pagefault 微基准触发异常路径。需对照日志定位（缺页处理 `mm-impl/impl-sv39/src/user_heap_mmap.rs:121-144`）。
- 收益：1~2 分。

### G6. iozone 写未过线（RV 尤甚）

- **现象**：RV 写类（fwriters/pwrite/initial writers/rewriters）result 1100~1800，baseline 3200~3700，全 1.0（行 230-274）；LA 写已 >baseline（1.3~1.45）。即 **RV 写仅需约 2x 即可翻线**，比 RV 读（需 ~20x）现实得多。
- **杠杆**：G2 的块缓存 + write-allocate + 增大块/批量写 + 缩短 `SharedRwFs` 全局 Mutex 临界区（`impl-fs-bridge/.../paged_handle.rs:383-384`）。
- **收益**：RV 写约 8 项，每项翻线 +0.x，合计可观；风险中。

### G7. lmbench 延迟项卡 1.0（syscall/read/write/stat/open/close/pipe/select）

要翻线需把这些延迟压到 baseline 以下（baseline 见 score.txt 行 591-667）：

| 项 | baseline(us) | 主要成本（当前代码） | 关键改法 |
|---|---|---|---|
| Simple syscall | 9.25 | RV trap 多次 TrapContext(296B) 拷贝 + 3~4 次**全局** `sfence.vma`（`impl-riscv64/asm/trap.asm:91,275-296`、`trap_handler.rs:132-133`）；返回查 signal 锁 | trap 单缓冲去冗余拷贝（H-1）；同 aspace 免全局 flush（M-1/M-2）；无 pending signal 快路径（H-6） |
| Simple read/write | 16.8 | ≈2× syscall 固定税 + 用户拷贝每页两次 walk（`mm-impl/impl-sv39/src/user_access.rs:81-117`）| 合并 translate+perm 单次 walk（H-2）+ 上面 trap 优化 |
| Simple stat | 425 | 无 dcache，全路径 `path_to_inode`（见 2.2）| dcache（F-3） |
| Simple open/close | 501 | 路径解析 + `exists`+`metadata` 双查 + fd 分配 O(n)（`impl-fd-session/src/registry.rs:291-294`）+ 页缓存 open_ref | dcache + 合并 exists/metadata + fd 空闲位图 |
| Pipe latency | 141 | 2× trap 链 + 调度 | 直接受 trap 优化 + pipe wake 策略 |
| Select 100 fd | 56 | `poll_engine.rs:495-523` 0..nfds 全扫 | 仅扫 fd_set 置位项 / 事件驱动 |

- **收益**：每项翻线 +1~2，是 lmbench 总分（现 144.9）最大的潜在增量来源；**但多数依赖中~高风险的 trap/TLB/COW 改造**，需 Feature Flag 灰度（见 `perf-risk-assessment.md`）。

### G8. 网络全部卡 1.0

- **位置**：`driver-network/src/lib.rs:141` 全局 `NETWORK_STACK: Mutex`；单次 TCP send 反复加锁 + `socket_send` 内部 `poll()` 与 syscall 层 poll **重复**（`lib.rs:677-691` 与 `sys/sendto.rs:148-156` 等）；smoltcp adapter 每 poll 仅收 1 帧、RX/TX buf=2048（`impl-smoltcp/src/lib.rs:14-15,81-129`）；阻塞循环 `sleep_for_ticks(1)` 空转。
- **改法**：去掉 `socket_send/recv` 内部重复 poll、单次锁内完成收发；`receive()` 一次 drain 多帧；RX/TX/UDP 缓冲扩到 ≥8~64KiB；send 路径减少 `Vec` 中转拷贝。
- **收益**：iperf TCP / netperf STREAM/RR 有望翻线，+10~20 分；风险中。

### G9. busybox kill / mv / rmdir = 0（功能）

- 行 121/126/133：`kill 10`、`mv test_dir test`、`rmdir test` 在 4 配置全 0。属功能性，疑似 rename/rmdir/信号投递语义问题；定位修复 ≈ 6 分。

---

## 4. fork+/bin/sh 920ms 专项（影响 lmbench Process 与 shell 类）

- **现象**：`Process fork+/bin/sh -c` baseline=920010us（行 594），是 fork+execve（2004us）的 ~460 倍。
- **因果链**：fork（整树页表复制）→ execve `/bin/sh`→`/glibc/busybox` 重定向（`sys/execve.rs:134-140`）→ `from_elf_path` 对 ~375 个 PT_LOAD 页 **eager 逐页 alloc+清零+读文件**（`mm-impl/impl-sv39/src/kernel_elf.rs:845-927`）→ parent wait/reap 持锁 `destroy_table`（`task-impl/impl-core/src/process.rs:779-803`）。
- **最大杠杆**：execve 改 **lazy file VMA**（复用已有 `register_lazy_file_vma` / `handle_lazy_page_fault` / `VfsMmapPageLoader`），exec 只建 VMA、首次缺页按需载入 → 920ms 有望降到数十 ms。其次：页表结构 COW（高风险）、reap 释锁后 destroy（中高）。
- **收益**：该项已 >1（rv-glibc 1.37），但 lazy map 还能进一步拉开，并大幅加速所有 shell/busybox/ltp 启动（间接提升功能类吞吐与稳定性）。

---

## 5. 风险 × 收益矩阵与实施顺序

| 序 | 改动 | 对应缺口 | 收益 | 风险 | 是否需 Flag |
|---|---|---|---|---|---|
| 1 | **定位并修复 LA-musl LTP=0** | G1 | **极高(~568)** | 取决于根因 | 否（功能修复）|
| 2 | **启用+扩容块缓存（RV 加 feature / LA 接线）** | G2,G6 | 高 | **低** | 构建 feature |
| 3 | ctx switch 不计 0（时间片/promote/队列 compact）| G3 | 高 | 低中 | 部分 |
| 4 | ext4 dcache + 句柄绑 inode + 去重复解析 | G2,G7 | 高 | 中 | 否 |
| 5 | 页缓存 O(1) LRU + install 去 alloc + 批量预取 | G2 | 中高 | 低中 | 否 |
| 6 | execve lazy file map | §4 | 中高 | 中 | 建议 |
| 7 | 网络去重复 poll + 缩锁 + 多帧 drain + 扩缓冲 | G8 | 中 | 中 | 否 |
| 8 | 修 regex_search / Pagefaults / busybox 0 分 | G4,G5,G9 | 中(功能) | 低中 | 否 |
| 9 | trap 单缓冲 / 选择性 TLB flush / 用户拷贝单 walk | G7 | 很高 | **高** | **必须** |
| 10 | fork 页表结构 COW / reap 释锁 destroy | G3,§4 | 高 | **高** | **必须** |

**建议节奏**：
- **第一波（低风险、高确定性）**：序 1（功能，单独并行）+ 序 2 + 序 3 + 序 8。块缓存与 ctx 计 0 是「确定能拿分且不易回归」的项。
- **第二波（中风险）**：序 4、5、6、7。每项独立 PR + `make rv_check && make la_check` + 定向 benchmark + LTP/busybox 全量回归。
- **第三波（高风险，灰度）**：序 9、10。必须 Feature Flag、默认旧路径，参照 `perf-risk-assessment.md` 的灰度与不变量断言要求。

---

## 6. 与第一版报告（`docs/todo/perf-*.md`）的关系

- 第一版按**子系统**枚举热点（hotpath/memory/fs-vfs/ipc-sync/lock-resource）并附风险评估；本报告按**评分缺口**重新组织，明确「哪些改动能真正翻过 baseline 拿到增量分」。
- 编号（H-x/M-x/F-x/I-x/L-x）沿用第一版，便于交叉索引；第 1 层已实施项（如 H-16 trace 门控、I-15 wait queue BTreeSet、F-14 顺序预取等）不再重复建议。
- 本报告新增的关键事实（第一版未明确）：
  1. **评分是 `max(1.0, 比值)`**——接近 baseline 不得分；
  2. **块缓存在评测构建里两架构都没生效**；
  3. **LA-musl LTP 整套 0 是全系统最大单缺口（功能性）**；
  4. **context switch / regex_search / Pagefaults / busybox 多项是「0 分失败」而非「慢」**，修复确定性高。

---

## 7. 待确认 / 需要你拍板的点

1. **LA-musl LTP=0（G1）**是否优先于性能优化推进？（收益远超所有性能项之和，但属功能修复）
2. 评测实际构建用的是 `os/Cargo.toml` 的哪个 feature 组合？需确认块缓存确未启用（本报告基于当前 main 的 feature 列表判断）。
3. 高风险项（trap 单缓冲、选择性 TLB flush、页表 COW）是否允许引入 Feature Flag 并安排灰度？
