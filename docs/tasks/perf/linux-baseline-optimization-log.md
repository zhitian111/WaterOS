# Linux baseline 优化执行日志

本文件持续记录以 BuildStorm 为主指标的优化任务、设计、验证结果和提交。阶段目标是
WaterOS 用时不超过同机 Linux baseline 的 2 倍，最终目标是不超过 Linux baseline。

## 固定基线与验证口径

- Linux RISC-V baseline：395.90 s（16 GiB、8 vCPU）。
- Linux LoongArch baseline：353.55 s（36 GiB、12 vCPU）。
- RISC-V 阶段门槛：791.80 s；最终门槛：395.90 s。
- 性能结论必须来自 Final profile、官方比赛镜像、与线上一致的 QEMU 参数，并使用
  `-snapshot`，确保每轮从同一磁盘状态启动。
- 每项优化至少完成双架构静态检查；保留性能改动前必须完整运行 RISC-V Final
  BuildStorm。涉及架构专属逻辑时补跑对应架构完整测试。

## MM-02A：消除重复 TLB 失效与只读 brk 查询失效

状态：已验证并回退（2026-08-10）

### 目标调用链

```text
sys_brk/sys_munmap/sys_mprotect/madvise
  -> with_user_aspace_mut_and_flush
     -> HeapBrk/MmapOps 修改页表
        -> fence_user_ptes/flush_address_space_translations
     -> flush_tlb_local(All)
     -> request_tlb_shootdown
```

当前部分页表修改在实现层执行一次全地址空间失效，聚合层随后再次执行本地全失效并
请求远端 shootdown。`brk(0)` 只读取当前 break，却也走相同的失效路径。BuildStorm
频繁创建进程和调整堆，这些同步操作会直接放大单核热路径成本。

后续 agent 可用下面一条命令恢复完整上下文：

```bash
codegraph explore "with_user_aspace_mut_and_flush HeapBrk::brk MmapOps::munmap MmapOps::mprotect madvise_discard_mapped_pages handle_cow_page handle_cow_fault flush_address_space_translations all production callers and call paths"
```

### 本轮设计

1. 对所有生产调用方已由 `with_user_aspace_mut_and_flush` 包裹的 `brk`、`munmap`、
   `mprotect`、`madvise` 页表修改，移除实现层重复的全地址空间 fence；由聚合层统一在
   锁外前完成一次本地失效和远端 shootdown。
2. `brk(0)` 改用无 flush 的只读地址空间访问，只查询当前 break。
3. RISC-V 与 LoongArch 实现保持对称；不改变映射、权限、错误码和 SMP 可见性语义。
4. COW 精确到单页的失效拆分作为候选扩展，只有在上述低风险改动验证后再做，避免将
   两种语义变化混进同一性能结论。

### 验收与回退条件

- 双架构 Final `make check`/构建通过，无新增 warning 或格式错误。
- RISC-V 16 GiB/8 vCPU `-snapshot` 完整跑完 BuildStorm，无 panic、SIGSEGV 或尾部停滞。
- 与本任务改动前的同机同配置基线比较；若变化落在运行噪声内，记录为中性并依据代码
  风险决定是否保留。若明确退化或出现语义回归，回退实现，不提交性能代码。
- 完成后在本节追加实测时间、差值、结论和提交号。

### 实测结果

- 改前：1023.91 s；改后：1033.79 s。
- 差值：+9.88 s（+0.97%）；两轮均完整结束，`ok=true`，无 panic/SIGSEGV。
- 结论：结果处于既有约 10 s 运行波动范围，无法证明对完整 BuildStorm 有收益。为避免
  累积无可测价值的复杂度，代码改动全部回退，不进入性能提交。
- 原始日志：`/tmp/wateros-mm02a-before-rv.log`、
  `/tmp/wateros-mm02a-after-rv.log`（本机临时文件，不提交）。

## MM-01A：评估并实现按需 brk（候选）

状态：采样后终止（2026-08-10）

### 目标调用链

```text
sys_brk
  -> HeapBrk::brk
     -> map_zeroed_page_with_alloc (增长区间逐页分配、清零、建 PTE)

user page fault
  -> MmapOps::handle_page_fault
     -> handle_brk_page_fault
        -> map_zeroed_page_with_alloc (现有按需零页路径)
```

恢复上下文命令：

```bash
codegraph explore "HeapBrk::brk handle_brk_page_fault user_brk_start user_brk_current_end user_brk_max initialization fork clone exec destroy and all brk tests; show call paths and exact relevant source"
```

### 候选设计与决策门槛

1. 候选方案参考 Linux 匿名堆：增长只校验范围并推进 `current_end`，首次读写由现有
   `handle_brk_page_fault` 分配清零页；收缩继续只回收已驻留页。
2. fork 已复制 `user_brk_{start,current_end,max}`，未驻留页无需额外复制；exec 创建新地址
   空间，不引入额外生命周期状态。
3. 当前 `api-v0::HeapBrk` 文档明确要求增长时立即分配映射。实施 lazy 方案需要同步放宽
   稳定契约，属于 API 语义变化，不能仅凭“已有 fault handler”直接修改。
4. 先用 pc-hot 的 `fast=1` 对完整测试前 300 s 采样。只有 brk 零页、帧分配或相关页表
   路径构成显著热点时才实施；否则终止该候选并转向采样排名更高的内存路径。

### 采样结论

- 300 s、8 vCPU、`fast=1` 共采样 225,996,010,249 条指令。
- brk 零页和帧分配路径未进入 Top 80；直接改 lazy brk 缺乏收益证据，且需要改变
  `api-v0` 的 eager 映射契约，因此不实施。
- `Sv39AddressSpace::mprotect` 被归并为第一热点。原始 PC `0x8026f8a0` 等确认落在
  `lazy_vma_overlaps` 的线性 VMA 扫描循环；仅该循环的五条核心指令各执行
  3,596,178,512 次，合计约占总采样指令的 7.96%。
- 原始采样：`/tmp/wateros-current-rv-pcs.txt`；Top 80：
  `/tmp/wateros-current-rv-pchot-top80.txt`（本机临时文件，不提交）。

## MM-02B：lazy VMA 重叠查询改为二分定位

状态：已完成（2026-08-10）

### 具体模块与调用链

- 模块：`wateros-mm-impl-sv39`、`wateros-mm-impl-loongarch64` 的 `pagetable.rs`。
- 热链：`sys_mprotect -> MmapOps::mprotect -> lazy_vma_overlaps -> Vec::iter().any()`。
- 同一查询还被 lazy mmap 注册、brk 冲突检查、brk fault 和 mremap 使用。
- CodeGraph 恢复命令：

```bash
codegraph explore "protect_lazy_file_vmas lazy_vma_overlaps lazy_file_vma_index insert_lazy_file_vma mprotect sys_mprotect exact source and all callers; sorted invariant"
```

## FS-02B：ext4 内层块缓存命中使用写时复制

状态：已完成（2026-08-10）

### 证据与调用链

当前 Final 的 300 s pc-hot 中，`another_ext4::BlockCache::read_block` 内 4 KiB
`Block::clone` 的直达 `memcpy` callsite 执行 6,804,463 次。当前命中路径为：

```text
VFS lookup/read/metadata
  -> Ext4::read_block
     -> another_ext4::BlockCache::read_block
        -> 命中持有 Box<[u8; 4096]> 的 CacheSlot
        -> Block::clone
           -> TLSF 分配 4096 bytes
           -> memcpy 4096 bytes
```

WaterOS 外层 `CachingBlockDevice` 缓存 512-byte LBA；another_ext4 内层缓存 4 KiB ext4
块并承担 dirty/write-back 语义。直接禁用内层缓存会增加锁和适配层调用，且改变写回行为，
不作为首选。

### 设计与备选

1. 首选：`Block` 数据改为引用计数所有权；cache hit 的 clone 仅增加引用计数。所有修改
   入口通过 `Arc::make_mut` 写时复制，读路径保持共享，写回和 LRU 锁边界不变。
2. 保留 `BlockDevice` 和 `Ext4::read_block -> Block` 接口，避免把 cache guard 生命周期
   传播到目录、extent、inode 和 xattr 全链路。
3. 若引用计数成本抵消收益，备选是增加 `read_block_into` 并让调用方复用缓冲，只消除
   TLSF 分配但仍保留 4 KiB copy；需要逐个改造消费者。
4. 更激进的借用式 cache guard 可完全去掉 copy，但会让解析期间长期持有全局 cache 锁，
   并要求 pin/eviction/write exclusion 语义；当前缺少这些基础设施，暂不采用。

恢复上下文命令：

```bash
codegraph explore "another_ext4 BlockCache::read_block CacheSlot Block data clone Arc make_mut Ext4::read_block all read and mutation callers; BlockAdapter and outer CachingBlockDevice"
```

### 验收

- another_ext4 单元测试覆盖 cache hit 共享、修改时分离、dirty flush 内容正确。
- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 明确优于 926.21 s。
- 改后反汇编或 pc-hot 确认原 4 KiB clone 的分配和 memcpy callsite 消失。

### 验证结果

- another_ext4 单元测试：2/2 通过；cache 测试确认两次读取只访问后端一次、命中共享数据、
  修改时分离，且 flush 写出修改后的内容。
- 双架构 Final check/build：通过。
- RISC-V Final 反汇编中，`BlockCache::read_block` 已不再调用 `memcpy` 或为 clone 申请
  4096 bytes。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，908.02 s；无
  panic/SIGSEGV，完整结束。
- 相对 926.21 s 对照减少 18.19 s（1.96%）；相对 Linux 395.90 s 为 2.29 倍，距离
  2 倍阶段门槛 791.80 s 尚差 116.22 s。
- 结论：消除 cache-hit 的 4 KiB TLSF 分配和复制有可复现价值，保留该实现。收益没有
  callsite 次数暗示得大，说明 BuildStorm 的主要剩余差距仍分布在路径处理、用户复制、
  inode/extent 解析和内存管理，而非单一块复制。
- 完整日志：`/tmp/wateros-fs02b-after-rv.log`（本机临时文件，不提交）。

## PATH-01A：已规范化绝对路径快速复制

状态：已完成（2026-08-10）

### 证据与调用链

FS-02B 后用当前 Final ELF 重跑 300 s pc-hot：`normalize_absolute_path` 聚合
683,545,098 条指令，函数内 `memcpy` callsite 执行 8,994,172 次。调用链覆盖 openat、
metadata、mount route 和 symlink resolve：

```text
copy_user_path_cstr / cwd + relative path
  -> resolve_path_at / VFS entry
     -> normalize_absolute_path
        -> split('/')
        -> 逐分量判断 empty / . / ..
        -> 逐段 push 到新 String
```

BuildStorm 的编译器路径绝大多数已经是标准绝对路径，但每次仍执行完整分量状态机。

### 设计

1. 增加无分配的单次 byte scan；确认路径以 `/` 开头、非根路径不以 `/` 结尾，且没有
   空、`.` 或 `..` 分量。
2. 命中时直接 `String::from(path)` 构造 `NormalizedPath`；不逐段 push，也不改变 UTF-8
   字节。
3. 未命中继续走原实现，保持根以上 `..` 折叠、重复斜杠、尾斜杠及错误语义。
4. 不引入 Linux dentry/namei cache：当前缺少 dentry 生命周期、rename invalidation 和
   mount namespace generation；本项只优化纯函数。

恢复上下文命令：

```bash
codegraph explore "wateros_vfs_api_v0::path::normalize_absolute_path NormalizedPath all callers resolve_path_at mount route symlink exact source and pathname invariants"
```

### 验收

- 单元测试覆盖根、标准路径、重复斜杠、`.`、`..`、尾斜杠与 UTF-8。
- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 明确优于 908.02 s，否则回退。

### 验证结果

- wateros-vfs-api-v0 单元测试：5/5 通过；覆盖根、标准路径、重复斜杠、`.`、`..`、
  尾斜杠和 UTF-8。
- 双架构 Final check/build：通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，900.64 s；无
  panic/SIGSEGV，完整结束。
- 相对 908.02 s 对照减少 7.38 s（0.81%）；相对 Linux 395.90 s 为 2.28 倍，距离
  2 倍阶段门槛 791.80 s 尚差 108.84 s。
- 结论：标准绝对路径占比足以抵消一次预扫描成本，保留快路径；收益有限，后续不继续
  堆叠字符串微优化，应转向 copy_from_user、页表和高层路径重复解析。
- 完整日志：`/tmp/wateros-path01a-after-rv.log`（本机临时文件，不提交）。

## COPY-02A：per-CPU 发布当前用户地址空间指针

状态：完整测试超时并回退（2026-08-10）

### 证据与调用链

当前 pc-hot 中，`Sv39UserMemoryOps::copy_from_user` 聚合 792,143,258 条指令；同时
`process_task_snapshot`、`task_snapshot`、`current_task_snapshot` 等完整快照路径合计超过
十亿条指令。用户复制获取地址空间的公共链路为：

```text
copy_from_user / copy_to_user
  -> current_user_aspace_handle
     -> task::current_task_user_aspace_ptr
        -> scheduler::current_task_snapshot
           -> 全局 scheduler lock
           -> registry.task_snapshot + 完整 TaskSnapshot
           -> live tick / vruntime 补算
        -> 只读取 user_aspace_ptr 一个字段
```

scheduler 已用 per-CPU `CURRENT_TASK_IDS` 原子槽避免当前 task-id 查询递归获取全局锁；当前
地址空间也由 `CPUState` 在 switch/exec 时同步维护，具备相同的发布条件。

### 设计

1. 在 scheduler impl 增加 per-CPU `CURRENT_ASPACE_PTRS`，与 `CURRENT_TASK_IDS` 在
   `with_scheduler` 的统一尾部从 `CPUState::current_aspace()` 发布。
2. 新增 impl 内部 `current_task_user_aspace_ptr()`：关闭本地中断后选择当前 CPU 槽，使用
   Acquire load；0 继续表示 idle/kernel/no user aspace。
3. task 聚合层现有同名接口改为直接调用，不扩大 `task-api/api-v0` 稳定接口。
4. 不改变地址空间 enter/leave 通知、SATP 切换、任务迁移或调度策略；该缓存仅是当前
   scheduler 状态的只读镜像，沿用 `CURRENT_TASK_IDS` 的一致性模型。

恢复上下文命令：

```bash
codegraph explore "current_task_user_aspace_ptr current_user_aspace_handle CURRENT_TASK_IDS with_scheduler CPUState::current_aspace set_current_task execve_current all context-switch publish paths"
```

### 验收

- task/scheduler 现有测试与双架构 Final check/build 通过。
- Final feature tree 不启用 user-copy diagnostics。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 明确优于 900.64 s，否则回退。

### 验证结果与结论

- 双架构 Final check/build：通过；普通 Final feature tree 不含 `user-copy-diagnostics`。
- RISC-V 完整 BuildStorm 在 1800 s 上限超时，未到达 `BUILDSTORM_COMPILE`；未发现
  panic、SIGSEGV 或明确 EFAULT。
- 改动全部回退。`CURRENT_TASK_IDS` 的统一尾部发布适合 scheduler 条件查询，但地址空间
  指针参与 exec/switch 后的首个用户访问；上下文切换可能不在新任务栈上返回到统一尾部，
  因而不能假设二者具有完全相同的发布时序。
- 后续若重做，必须在 `set_current_task`、exec 更新和进入 idle 的精确状态变更点发布，并
  增加“切换后首个 syscall 返回正确 aspace”的双架构运行测试；在这些基础设施完成前，
  保留完整快照查询的正确性路径。
- 超时日志：`/tmp/wateros-copy02a-after-rv.log`（本机临时文件，不提交）。

## FS-03A：挂载时缓存 ext4 不可变几何参数

状态：完整测试退化并回退（2026-08-10）

### 证据与调用链

当前 profile 中 `inode_disk_pos` 聚合 167,173,736 条指令，`read_inode` 为 77,870,676
条。一次 inode 读取包含两次 superblock cache access：

```text
read_inode
  -> inode_disk_pos
     -> read_super_block                  # 第一次
     -> read_block_group
        -> block_group_disk_pos
           -> read_super_block            # 第二次
```

每次 `read_super_block` 都进入 another_ext4 全局 block-cache mutex、执行 set/LRU 查找并从
block 0 解析 1024-byte `SuperBlock`。其中 inode/block-group 定位只需要四个挂载期间不变
的几何字段。

### 设计

1. `Ext4::load` 校验 superblock 后保存 `inodes_per_group`、`inode_size`、`desc_size` 和
   `first_data_block` 到只读 `Ext4Geometry`。
2. `inode_disk_pos` 与 `block_group_disk_pos` 直接读取几何字段，消除每次 inode 定位的
   两次 block-cache access 和 superblock 反序列化。
3. free block/inode counters、checksum、UUID 等状态继续经 `read_super_block`/
   `write_super_block`，不缓存可变数据，不改变 allocation 与 write-back 语义。
4. 不引入 Linux inode/dentry cache；当前缺少 rename/unlink invalidation 和 inode
   reclaim 基础设施，本项仅缓存 ext4 格式定义为静态的布局参数。

恢复上下文命令：

```bash
codegraph explore "another_ext4 Ext4::load read_inode inode_disk_pos read_block_group block_group_disk_pos read_super_block write_super_block all callers and geometry mutation semantics"
```

### 验收

- another_ext4 测试通过，反汇编确认 `inode_disk_pos` 不再调用 `read_super_block`。
- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 明确优于 900.64 s，否则回退。

### 验证结果与结论

- another_ext4 单元测试：2/2 通过；双架构 Final check/build 通过。
- RISC-V 反汇编确认改动版 `inode_disk_pos` 不再调用 `read_super_block/read_block`。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，917.11 s；无
  panic/SIGSEGV，完整结束。
- 相对 900.64 s 对照增加 16.47 s（1.83%），属于明确退化；代码全部回退。
- 结论：FS-02B 后 superblock cache hit 已是共享数据，显式访问的墙钟代价低于函数指令
  计数所暗示；新增 `Ext4` 字段及不同代码布局反而恶化 TCG 执行。后续不继续缓存零散
  ext4 几何字段，除非先用调用级计时证明锁等待占比。
- 完整日志：`/tmp/wateros-fs03a-after-rv.log`（本机临时文件，不提交）。

## COPY-02B：仅在调度状态变更点发布当前地址空间

状态：完整测试退化并回退（2026-08-10）

### COPY-02A 失败修正

CodeGraph 确认所有实际任务切换均通过 `MultiClassScheduler::set_current_task`，它在
`__switch` 前更新 `CPUState.current_aspace`；idle 任务也走同一入口。exec 则在
`execve_current` 内单独更新当前 CPU 的 aspace。COPY-02A 把原子发布放在通用
`with_scheduler` 尾部，导致每一次 scheduler 查询/操作都执行 Release store，热点快照
查询本身因此持续写共享 cache line，可能造成严重伪共享，而非必要的状态同步。

### 设计

1. 保留 per-CPU `CURRENT_ASPACE_PTRS`，但只由 `set_current_task` 和 exec 的实际 aspace
   变更点调用发布函数；不在通用 `with_scheduler` 尾部写入。
2. 发布发生在 `__switch` 前，首次任务、普通切换、block/exit 后选择 idle 均覆盖；exec
   在替换 `CPUState` 后立即发布。
3. 查询关闭本地中断后 Acquire load CPU-local 槽，不再构造 `TaskSnapshot`。
4. 不改变 SATP、enter/leave 通知、迁移和调度策略。若短启动或完整测试异常立即回退。

恢复上下文命令：

```bash
codegraph explore "MultiClassScheduler::set_current_task execve_current CPUState::set_current_task current_aspace switch_and_unlock __switch all callers and ordering"
```

### 验收

- 短快照启动能够进入并持续执行 BuildStorm，无 EFAULT/panic。
- 双架构 Final check/build 通过。
- 完整 RISC-V BuildStorm 明确优于 900.64 s，否则回退。

### 验证结果与结论

- 180 s RISC-V `-snapshot` smoke：通过 toolchain/minibuild 并进入正式 BuildStorm 编译，
  无 EFAULT/panic。
- 双架构 Final check/build：通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，908.65 s；无
  panic/SIGSEGV，完整结束。
- 相对 900.64 s 对照增加 8.01 s（0.89%）；代码全部回退。
- 结论：仅在真实状态变更点发布修复了 COPY-02A 的超时，但 per-CPU Release/Acquire、
  单独查询函数和中断守卫的净成本仍未转化为墙钟收益。连续两种 aspace 镜像方案均失败，
  后续停止该方向，优先优化用户复制内部页表遍历或减少上层复制次数。
- smoke 日志：`/tmp/wateros-copy02b-smoke-rv.log`；完整日志：
  `/tmp/wateros-copy02b-after-rv.log`（本机临时文件，不提交）。

## COPY-03A：用户路径按页批量复制并扫描 NUL

状态：完整测试超时并停止（2026-08-10）

### 证据与调用链

`copy_user_path_cstr` 被 38 个 syscall 路径调用。它只捕获一次 aspace handle，但当前按
单字节循环调用用户复制：

```text
copy_user_path_cstr
  -> for each byte
     -> ActiveUserMemoryOps::copy_from_user([u8; 1])
        -> with_user_aspace_mut
        -> translate_addr_with_perm（三级页表 walk）
        -> permission check
        -> memcpy 1 byte
```

当前 pc-hot 中 `copy_user_path_cstr` 聚合约 88,973,767 条指令；普通构建路径通常几十到
数百字节且落在同一用户页，页表遍历次数可降低一个数量级。

### 设计

1. 每轮计算当前用户地址到页末的长度，批量复制 `min(page_room, max-len)` 字节到已分配
   的目标 `Vec`。
2. 只在已验证的单个用户页内预取，然后在复制结果中搜索 NUL；命中立即 truncate。
3. 不跨页预取，保证 NUL 位于页尾前时不会因后一页未映射而错误返回 EFAULT；每个新页
   仍执行 fault、U/R 权限检查。
4. 用 `checked_add` 处理地址溢出；保留 `ENAMETOOLONG`、UTF-8 和空字符串语义。
5. 不缓存 VA→PA 翻译，不需要页表失效或 aspace 生命周期基础设施。

恢复上下文命令：

```bash
codegraph explore "copy_user_path_cstr ActiveUserMemoryOps::copy_from_user user_copy copy_from_user_in_aspace translate_addr_with_perm all callers exact source and C string fault semantics"
```

### 验收

- 单元/内核用户复制测试覆盖同页、跨页、NUL 后未映射页、无 NUL 和地址溢出。
- 双架构 Final check/build 通过。
- 完整 RISC-V BuildStorm 明确优于 900.64 s，否则回退。

### 验证结果与结论

- 双架构 Final check/build：通过；180 s snapshot smoke 通过 toolchain/minibuild 并进入
  正式编译，无 EFAULT/panic。
- 完整 RISC-V BuildStorm 在 1800 s 上限超时，未到达 `BUILDSTORM_COMPILE`，无
  panic/SIGSEGV。
- 根因：普通路径通常仅几十字节，但按页方案会从起始地址复制最多近 4 KiB，再扫描
  NUL；减少页表 walk 的收益远小于过量 memcpy/memchr，导致灾难性退化。
- 页级批量方案停止，不提交。下一实验限制为 64-byte 且不跨页的小窗口，使短路径只
  产生有限过读，同时仍把页表 walk 从逐字节降到每 64 bytes 一次。
- smoke 日志：`/tmp/wateros-copy03a-smoke-rv.log`；超时日志：
  `/tmp/wateros-copy03a-after-rv.log`（本机临时文件，不提交）。

## COPY-03B：用户路径使用 64-byte 页内窗口

状态：完整测试退化并回退（2026-08-10）

### 设计

1. 每轮复制 `min(64, page_room, max-len)`，随后在该窗口扫描 NUL。
2. 典型短路径只需一次 aspace cell 获取和页表 walk；跨页路径仍严格在新页重新验证。
3. 相对 COPY-03A，最坏无用读取从接近 4095 bytes 限制为 63 bytes；相对原实现，64
   字节路径的页表 walk 从 64 次降为 1 次。
4. 保持地址溢出、UTF-8、ENAMETOOLONG、空字符串和 NUL 后未映射页语义。

恢复上下文命令：

```bash
codegraph explore "copy_user_path_cstr ActiveUserMemoryOps::copy_from_user fixed window page boundary NUL semantics exact source"
```

### 验收

- 双架构 Final check/build 与 180 s snapshot smoke 通过。
- 完整 RISC-V BuildStorm 明确优于 900.64 s，否则恢复逐字节实现。

### 验证结果与结论

- 双架构 Final check/build：通过；180 s snapshot smoke 通过 toolchain/minibuild 并进入
  正式编译，无 EFAULT/panic。
- 完整 RISC-V BuildStorm：`ok=true`，925.56 s；无 panic/SIGSEGV，完整结束。
- 相对 900.64 s 对照增加 24.92 s（2.77%），明确退化；逐字节实现已恢复。
- 结论：限制过读后仍退化，说明批量 memcpy 后再扫描 NUL 的双遍访存和新增分支成本
  超过页表 walk 减少的收益。pc-hot 指令占比不能直接换算 TCG 墙钟；停止 C 字符串批量
  复制方向，后续应减少调用链上的复制次数而不是扩大每次复制。
- smoke 日志：`/tmp/wateros-copy03b-smoke-rv.log`；完整日志：
  `/tmp/wateros-copy03b-after-rv.log`（本机临时文件，不提交）。

## COPY-01A：RISC-V 对齐 memcpy 64 字节展开

状态：完整测试显著退化并回退（2026-08-10）

### 模块与热点

- 模块：`wateros-platform/platform-arch/impl-riscv64`，内核 C ABI `memcpy`。
- MM-02C 后 300 s pc-hot 中，`compiler_builtins::mem::memcpy` 合计约 254.24 亿条指令。
- 其中对齐 8 字节循环的 `ld/sd/add/add/branch` 五条指令各执行 4,642,413,418 次，合计
  约 232.12 亿条；非对齐字拼接循环的核心指令各仅约 816 万次。
- `memcpy` 入口约 6,138 万次，对齐循环约 46.42 亿轮，平均每次调用约 75 个双字，即
  约 600 字节。当前实现每 8 字节一次分支，循环控制成本过高。

### 设计

1. RISC-V 提供强 C ABI `memcpy`：保存原始 `dst` 作为返回值。
2. 小块、无法把 src/dst 同时对齐的复制使用字节循环；共同 8 字节对齐后，每轮展开
   8 组 `ld/sd`，复制 64 字节，再处理 8 字节与字节尾部。
3. 不执行未对齐 `ld/sd`，不依赖 QEMU 的未对齐访问模拟；不修改 `memmove`，调用方仍须
   遵守 `memcpy` 源目标不重叠契约。
4. LoongArch 暂时保持 compiler-builtins 实现；其完整构建用于防止共享接口回归。
5. 构建后用 `llvm-nm` 与 `llvm-objdump` 确认内核实际导出并调用新实现；若未替换成功，
   不进入完整性能测试。

恢复上下文命令：

```bash
codegraph explore "RISC-V platform arch global_asm memcpy compiler_builtins copy_from_slice GlobalFilePageCache install_page read_key user_copy exact source and callers"
```

### 验收

- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 成功，无 panic/SIGSEGV。
- 相对 926.21 s 对照有明确收益；改后 pc-hot 中 memcpy 循环控制指令显著减少。

### 验证结果

- 新实现成功替换 compiler-builtins：`llvm-nm` 显示唯一强符号 `memcpy T 8033cfa4`，
  `llvm-objdump` 显示内核调用点均跳转至该实现。
- 双架构 Final check/build：通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，1131.62 s；无
  panic/SIGSEGV，完整结束。
- 相对 926.21 s 对照增加 205.41 s（22.18%），属于确定性严重退化；汇编实现及接入
  全部回退，仅保留分析记录。
- 结论：pc-hot 指令数不能直接代表 QEMU TCG 的墙钟成本。64 字节展开减少 guest 分支，
  但更长基本块和更多活跃临时寄存器在当前 QEMU 上代价更高。后续 COPY 优化应消除跨层
  复制次数或直接填充最终页，而非替换通用 memcpy。
- 完整日志：`/tmp/wateros-copy01a-after-rv.log`（本机临时文件，不提交）。

## COPY-01B：page-cache miss 直接填充最终槽位

状态：完整测试无可证明收益并回退（2026-08-10）

### 模块与调用链

```text
GlobalFilePageCache::read_key / VfsMmapPageLoader::load_page
  -> GlobalFilePageCache::install_page
     -> [u8; 4096] page_buf
     -> PageCacheIo::read_range(page_buf)
     -> cache.page_data_mut(slot).copy_from_slice(page_buf)
```

当前 cache payload 是构造后不再扩容的连续 `Vec<u8>`，但所有权元数据只有
free/index/LRU 三态。若仅把槽从 LRU 摘下后锁外写入，`reset_to_gen` 或其他 miss 可能把它
当作 free 重用，造成数据竞争。

### 设计

1. `PageFrame` 增加内部 `reserved` 状态。预留槽不在 index、LRU 或 free 中，只有发起
   miss 的同步 I/O 路径能写其 payload。
2. miss 优先从 free 或 clean LRU 预留槽；锁外将槽清零并把其稳定 4 KiB payload 直接传给
   `PageCacheIo::read_range`，成功后在锁内一次发布 key/index/LRU。
3. I/O 失败、同页并发 miss 的后到者均取消预留并归还 free；dirty victim 继续走现有
   保存副本、锁外写回和 version 校验路径。
4. `reset_to_gen` 在清空连续 payload 元数据前等待 reserved 槽归还；循环不持锁等待，保证
   I/O 完成路径可以取得 state lock。
5. 单元测试除数据和 LRU 不变量外，直接比较 `read_range` 收到的 buffer 地址与 reserved
   cache payload 地址，证明不是仍经临时数组复制。

恢复上下文命令：

```bash
codegraph explore "GlobalFilePageCache install_page PageFrame GlobalCacheState pop_free_or_lru_index reset_to_gen clear_in_place page_data_mut PageCacheIo tests exact source and invariants"
```

### 验收

- page-cache 单元测试通过，覆盖直接地址、I/O 错误回滚和 LRU 不变量。
- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 成功且明确优于 926.21 s。

### 验证结果

- page-cache 单元测试：15/15 通过。新增测试确认 `PageCacheIo::read_range` 的目标地址就是
  reserved cache payload，并确认 I/O 失败归还槽位且 LRU 不变量成立。
- 双架构 Final check/build：通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，925.80 s；无
  panic/SIGSEGV，完整结束。
- 相对 926.21 s 对照仅减少 0.41 s（0.04%），完全处于运行噪声内。由于方案引入锁外
  raw payload、reserved 状态和 reset 等待，风险与复杂度不能由收益证明；实现和测试全部
  回退，仅保留分析记录。
- 结论：page-cache miss 最终一次 4 KiB staging copy 不是当前阶段缺口的主要来源。
  下一项应重新采样并优先评估更高频的 cache-hit → read lease → user-copy，或 mmap
  fault 中 page-cache → 用户 frame 的全页复制。
- 完整日志：`/tmp/wateros-copy01b-after-rv.log`（本机临时文件，不提交）。

## FS-02A：ext4 目录查找原地比较名称

状态：完整测试退化并回退（2026-08-10）

### 证据与调用链

使用当前有效 Final ELF 重跑 300 s pc-hot，并将所有直达 `memcpy` 的 callsite PC 与采样表
关联。最高的两个调用点均位于 `another_ext4::Ext4::dir_find_entry`，各执行 9,514,168 次：

```text
lookup / open / metadata
  -> Ext4::dir_find_entry
     -> DirBlock::get
        -> Block::read_offset_as::<DirEntry>
           -> DirEntry::from_bytes
              -> 清零 255-byte name
              -> memcpy 当前 name 到 DirEntry
        -> 将 256-byte DirEntry 再复制到扫描局部变量
        -> compare_name
```

普通文件 staged read 没有进入最高频 memcpy callsite，故暂停需要 cache pin/refcount 的
COPY-01C。其他高频调用点包括 `normalize_absolute_path` 8,994,189 次、另一个 ext4 块复制
6,804,463 次和 `copy_from_user` 4,947,611 次。

### 设计

1. `DirBlock::get` 直接从 4 KiB block payload 解码 8-byte ext4 dirent header。
2. 先校验 `rec_len >= 8`、记录不越过 block、`name_len <= rec_len - 8`；无效记录返回
   `None`，同时避免原实现可能在 `rec_len == 0` 时死循环。
3. inode 非零且长度相等时，直接把 block 内 name slice 与目标 `str::as_bytes()` 比较；
   仅返回 32-bit inode，不构造 `DirEntry` 或 `[u8; 255]`。
4. `list/insert/remove` 保留拥有型 `DirEntry` 路径，避免扩大补丁；双架构均使用同一
   little-endian ext4 解析。

恢复上下文命令：

```bash
codegraph explore "another_ext4 Ext4::dir_find_entry DirBlock::get DirEntry::from_bytes compare_name Block::read_offset_as all callers and exact source"
```

### 验收

- another_ext4 单元测试覆盖命中、未命中、unused 和损坏 rec_len/name_len。
- 双架构 Final check/build 通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 明确优于 926.21 s。
- 改后 pc-hot 中原 0x802fffa6/0x802fffb6 两个 memcpy callsite 消失。

### 验证结果

- another_ext4 单元测试：3/3 通过；新增用例覆盖命中、unused、未命中和损坏记录。
- 双架构 Final check/build：通过。
- 构建后反汇编确认 `dir_find_entry` 的两个 `memcpy` 和 255-byte `memset` 消失，仅保留
  长度匹配候选的 `memcmp`。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，941.62 s；无
  panic/SIGSEGV，完整结束。
- 相对 926.21 s 对照增加 15.41 s（1.66%），属于明确退化；vendor 实现和新增测试全部
  回退，仅保留分析记录。
- 结论：逐字段边界检查、little-endian 解码及 slice 逻辑在 QEMU TCG 上比原固定布局
  反序列化更贵。高频 memcpy callsite 数量也不能单独代表墙钟占比；后续选点必须结合
  函数内部总指令或 syscall 时间，而不是只看调用次数。
- 完整日志：`/tmp/wateros-fs02a-after-rv.log`（本机临时文件，不提交）。

## FILE-01A：普通 ext4 数据写不再隐式全盘 flush

状态：完整测试无收益并回退（2026-08-10）

### 模块与热链

```text
PagedFileHandle::write / close writeback
  -> GlobalFilePageCache::flush_key（最多合并 64 个连续脏页）
     -> FsPageIo::write_range
        -> AnotherExt4Fs::write_range_node
           -> write_with_ordered_size
              -> Ext4::setattr + flush_all（扩展写）
              -> Ext4::write + flush_all（所有写）
                 -> BlockAdapter -> CachingBlockDevice -> VirtIOBlk
                    -> add_notify_wait_pop（busy-poll）
```

MM-02C 后 300 s pc-hot 中，VirtIO `add_notify_wait_pop` 约 30.18 亿条指令。当前没有块设备
IRQ 注册、请求等待队列和可跨锁存活的 DMA buffer；直接把 busy-poll 改成 task sleep 会让
任务持有 `SharedBlockDevice` 的 spin mutex 睡眠，使中断完成路径无法安全接管设备。

### 设计

1. 普通 buffered data write 中，文件扩展仍先更新内存中的 `i_size`，随后写数据，但不在
   每次 `setattr` 或 `write` 后调用 `flush_all`。
2. 保留 `ReadWriteFs::sync` / `fsync` 的显式 `flush_all`，并保留 unlink/rename/orphan 等
   元数据操作现有同步策略。本轮不扩大崩溃一致性承诺：与 Linux 一样，未调用 fsync 的
   普通 write/close 不保证掉电后持久化。
3. `write_regular_file` 移除 helper 之外重复的 flush；其数据仍可由后续显式 sync 持久化。
4. 不在本轮实现 VirtIO 中断异步。后续 BIO 任务需先引入请求所有权、稳定 DMA buffer、
   释放设备锁后等待、IRQ ack/complete、取消与 teardown 协议。

恢复上下文命令：

```bash
codegraph explore "AnotherExt4Fs write_with_ordered_size write_range_node write_range write_regular_file sync; PagedFileHandle writeback_dirty sync_dirty; GlobalFilePageCache flush_key; BlockAdapter CachingBlockDevice VirtIOBlk add_notify_wait_pop exact source and call paths"
```

### 验收

- 双架构 Final check/build 通过。
- RISC-V 16G/8 vCPU、`-snapshot` 完整 BuildStorm 成功，无 panic/SIGSEGV。
- 相对 926.21 s 对照有可重复收益；若语义回归或性能退化则回退。

### 验证结果

- 双架构 Final check/build：通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：`ok=true`，937.39 s；无
  panic/SIGSEGV，完整结束。
- 相对 926.21 s 对照增加 11.18 s（1.21%），既无收益也未达到 Linux 2 倍的
  791.80 s 阶段门槛；代码改动全部回退，仅保留分析记录。
- 结论：普通数据写中的强制 flush 并非当前 BuildStorm 的主要瓶颈，不能仅凭 VirtIO
  busy-poll 的 30.18 亿指令归因于该写路径。下一次块层实验应先按请求类型、读写方向、
  LBA 连续性和请求尺寸计数，再决定 read-ahead、跨调用合并或异步完成。
- 完整日志：`/tmp/wateros-file01a-after-rv.log`（本机临时文件，不提交）。

### 验证结果

- 双架构 Final check/build：通过。
- RISC-V 完整 BuildStorm：`ok=true`，926.21 s；无 panic/SIGSEGV，完整结束。
- 相对 MM-02B 的 989.57 s：减少 63.36 s（6.40%）；相对初始 1023.91 s 累计减少
  97.70 s（9.54%）。当前为 Linux baseline 的 2.34 倍，距阶段门槛尚差 134.41 s。
- 改后 300 s pc-hot 共采样 120,021,740,857 条指令；`mprotect` 已从第一名退出 Top 40，
  证明全表 VMA 重建热点已消除。
- 新热点依次包括 `memcpy`（25,424,005,910）、TLSF allocate/deallocate、
  VirtIO `add_notify_wait_pop`（3,018,576,447）、`memset`、page-cache install/read、
  `find_free_mmap_base_considering_vmas` 和 `remove_lazy_file_vmas`。
- 完整日志：`/tmp/wateros-mm02c-after-rv.log`；采样：
  `/tmp/wateros-mm02c-rv-pcs.txt`（本机临时文件，不提交）。

## MM-03：mmap first-fit 逐页页表查询（暂缓）

状态：审计后暂缓（2026-08-10）

- pc-hot 的 387,012,821 条指令主要落在
  `find_free_mmap_base_considering_vmas` 对候选区间逐页 `translate_addr` 的循环。
- 该项只占同窗口总指令约 0.32%，完全消除的收益上限也很小。
- lazy file/shared anonymous VMA 已可按区间跳跃，但 private anonymous 映射没有完整 VMA
  记录；直接跳过页表检查会让后续非 fixed mmap 与既有 PTE 重叠。
- 结论：在补齐所有匿名映射区间语义前不做投机 fast path，优先更高占比热点。

## COPY-02：页缓存到 mmap 用户帧零拷贝（中期候选）

状态：完成归因，等待基础设施（2026-08-10）

- 300 s 采样中 `memcpy` 为 25,424,005,910 条指令（约 21.18%）。
- 文件页 miss 当前执行“块设备/FS → 4 KiB 栈缓冲 → page-cache frame”；文件 mmap fault
  再执行“page-cache frame → 新用户 frame”。
- 消除后一份复制需要页缓存帧可被用户 PTE 直接引用，并补齐 pin/refcount、只读共享、
  MAP_PRIVATE 写时复制、驱逐等待和地址空间销毁协议。当前 page cache 使用固定连续 `Vec<u8>`
  槽，不具备可独立映射帧的所有权模型。
- 结论：这是达到 Linux 最终目标的重要方向，但不以绕过缓存或裸借用指针的局部补丁实现。

## ALLOC-01A：削减 TLSF 同步元数据原子操作

状态：完整测试超时并回退（2026-08-10）

### 热点与模块

- 模块：`wateros-runtime/runtime-heap-allocator`。
- TLSF allocate/deallocate 及 GlobalAlloc 包装在 300 s 采样中合计约 90 亿条指令。
- 每次操作除全局 TLSF mutex 外，还对相邻的 per-CPU 递归深度槽执行 AMO，并用 CAS
  循环更新 `used_estimate`，造成不必要的原子 RMW 和潜在 cache-line 伪共享。

### 设计

1. 将每 CPU 深度槽按 64 字节对齐；中断关闭后槽由当前 CPU 独占，用 Relaxed load/store
   保留递归检测，替代 fetch_add/fetch_sub。
2. `used_estimate` 所有写路径统一置于 TLSF mutex 内，用 load + 饱和计算 + store 替代
   `fetch_update` CAS；无锁诊断读仍使用 AtomicUsize。
3. 保持 TLSF 算法、布局、全局互斥、OOM 统计和非法指针策略不变。

恢复上下文命令：

```bash
codegraph explore "wateros_runtime_heap_allocator InterruptSafeTlsfHeap GlobalAlloc alloc dealloc realloc with_allocator_interrupt_guard HeapMemStats TLSF initialization features small allocation cache slab all callers and exact source"
```

### 验证与结论

- 双架构 Final check/build：通过。
- RISC-V 完整 BuildStorm：1800 s 超时，未到达 `BUILDSTORM_COMPILE`；无 panic，但末段
  编译进度长时间停滞。对照版本为 926.21 s，属于确定性严重退化。
- 主要设计错误是把 dealloc 的用量估算更新从 TLSF 锁外移入全局锁，扩大所有 CPU 共享的
  allocator 临界区；减少 AMO 并不等于减少并发成本。
- 代码改动全部回退，不提交。后续 allocator 优化必须先具备分配尺寸/竞争计数，并优先
  设计真正缩短或绕过全局锁的 per-CPU cache，而不是调整锁内原子形式。
- 超时日志：`/tmp/wateros-alloc01a-after-rv.log`（本机临时文件，不提交）。

### 设计

`lazy_file_vmas` 在注册时按 `start` 插入，且拒绝重叠；拆分、删除和 fork 都保持顺序。
因此可先用 `partition_point(vma.end <= query.start)` 跳过所有位于查询左侧的 VMA，再只
检查第一个候选的 `start < query.end`。复杂度由 O(VMA 数) 降为 O(log VMA 数)，不改变
映射、权限、loader 生命周期或错误语义。双架构采用同一实现。

### 验收

- 双架构 Final check/build 通过。
- RISC-V 完整 Final BuildStorm 成功，且相对 1023.91 s 基线获得超出噪声的改善。
- 改后复跑同窗口 pc-hot，确认 `0x8026f8a0` 线性循环热点消失；若完整测试退化则回退。

### 验证结果

- `make check ARCH=rv PROFILE=final`：通过。
- `make check ARCH=la PROFILE=final`：通过。
- 双架构 Final build：通过。
- RISC-V 完整 BuildStorm：`ok=true`，989.57 s；无 panic/SIGSEGV，完整结束。
- 相对改前 1023.91 s：减少 34.34 s（3.35%）。相对 Linux 395.90 s 为 2.50 倍；
  距离 2 倍阶段门槛 791.80 s 尚差 197.77 s。
- 改后相同 300 s pc-hot 中，旧 `lazy_vma_overlaps` 线性扫描 PC 热环已消失；
  `mprotect` 聚合计数从 108,131,577,716 降至 86,669,001,515（-19.85%）。
- 新的主要内核热环位于 `protect_lazy_file_vmas`：每次 mprotect 仍 `drain(..)` 全量扫描并
  重建所有 VMA。其核心指令各执行约 1,418,596,177 次，是下一项 MM 优化候选。
- 改后完整日志：`/tmp/wateros-mm02b-after-rv.log`；改后采样：
  `/tmp/wateros-mm02b-rv-pcs.txt`（本机临时文件，不提交）。

## MM-02C：mprotect 仅更新相交 lazy VMA

状态：进行中（2026-08-10）

### 模块与热链

```text
sys_mprotect
  -> MmapOps::mprotect
     -> protect_lazy_file_vmas
        -> lazy_file_vmas.drain(..)
        -> 逐项 overlaps + 重建 Vec + duplicate_box
```

MM-02B 后的 pc-hot 显示，`protect_lazy_file_vmas` 全表循环的核心指令各执行约
1,418,596,177 次，绝大多数集中在同一 vCPU。当前实现即使请求不涉及 lazy VMA，也会
移动并重建整个向量；若 loader 复制中途失败，`drain(..)` 还会使原表部分丢失。

### 设计

1. 用 `partition_point(end <= start)` 和 `partition_point(start < end)` 得到相交的
   `[first,last)`；无交集立即返回。
2. 完整覆盖的 VMA 只原地更新 `perm`。
3. 首、尾部分覆盖时，在修改原表前预先复制所需的左/右 loader；复制全部成功后再调整
   原边界、批量更新权限，并最多各插入一个边界分片。
4. 中间 VMA 不复制、不移动；错误路径保持原表不变。RISC-V 与 LoongArch 保持对称。
5. 不引入红黑树、maple tree 或反向映射；这些 Linux 基础设施在当前实现中不存在，
   有序小向量上的二分和局部更新更直接。

恢复上下文命令：

```bash
codegraph explore "protect_lazy_file_vmas lazy_vma_overlaps lazy_file_vma_index insert_lazy_file_vma mprotect sys_mprotect exact source and all callers; sorted invariant"
```

## CACHE-03A：稳定文件页缓存使用轻量索引键

状态：完整测试无稳定收益并回退（2026-08-10）

### 模块与热链

```text
GlobalFilePageCache::{read_key,write_key,install_page,flush_key}
  -> GlobalCacheState.index: BTreeMap<(FileCacheKey, page_idx), slot>
     -> FileCacheKey::clone
        -> Arc<str> 原子引用计数
     -> FileCacheKey::cmp
```

pc-hot 中 `FileCacheKey::cmp` 约 4.58 亿条指令。ext4 文件已有稳定的
`(mount_id,node_id)` 身份，但当前每次 BTree 查询仍构造完整键并克隆路径 `Arc<str>`；单页
读写和安装会重复查询多次，路径本身在稳定键比较中并未使用。

### 设计与保留门槛

1. 为 `index` 引入只含 `mount_gen + identity + page_idx` 的内部键；稳定文件 identity 只保存
   两个整数，path-only 文件继续保存 `Arc<str>`，保持原有冲突与排序语义。
2. `PageFrame` 继续保存完整 `FileCacheKey`，因此 I/O、脏页回写、rename、truncate 和错误路径
   仍能取得原路径；不改变公开 API、页替换或一致性协议。
3. 添加稳定键忽略路径、路径键保持区分以及索引/LRU 不变量测试。
4. 双架构 Final check/build 后运行 RISC-V 16 GiB/8 vCPU、`-snapshot` 的完整 BuildStorm。
   以 900.64 s 为当前保留基线；无稳定改善或出现回归即回退代码，仅保留实验记录。

恢复上下文命令：

```bash
codegraph explore "FileCacheKey FileCacheIndexKey GlobalCacheState.index read_key write_key install_page install_zero_page flush_key purge_closed_file finish_rename truncate_key PageFrame exact source and all callers"
```

### 验证结果与结论

- 页缓存定向测试：14 项全部通过，包含新增的稳定键/路径键身份测试。
- `make check ARCH={rv,la} PROFILE=final` 与双架构 Final build：全部通过。
- RISC-V 完整 BuildStorm（16 GiB、8 vCPU、`-snapshot`）：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=903.72`，测试完整结束，无 panic/SIGSEGV。
- 相对 900.64 s 有效基线慢 3.08 s（+0.34%），没有达到预设的稳定改善保留门槛；代码
  已全部回退，仅保留本记录。测试日志：`/tmp/wateros-cache03a-after-rv.log`（本机临时文件）。
- 结论：稳定键查询中的路径 Arc 引用计数不是当前可独立兑现的主要瓶颈；4.58 亿条
  `FileCacheKey::cmp` 指令主要成本更可能来自 BTree 查找本身。后续若继续优化页缓存索引，
  应测量命中率和树深，并评估固定容量哈希/分片索引，而不是只缩减键的拥有字段。

## ALLOC-01B：融合 TLSF 分配统计与高水位检查

状态：双轮完整测试无稳定收益并回退（2026-08-10）

### 模块与热链

```text
GlobalAlloc::alloc
  -> with_allocator_interrupt_guard
     -> mem_stats
        -> used_estimate.load + pool_len.load
     -> TLSF mutex + allocate
     -> estimate_add CAS
  -> maybe_warn_high_water
```

TLSF 及其包装在 pc-hot 中是主要内存热点。当前普通分配为了只会打印一次的高水位告警，
每次都在真正分配前读取两个原子统计值，成功后又单独更新用量；告警看到的还是分配前数据。

### 设计与保留门槛

1. 让 `estimate_add` 返回饱和更新后的用量；成功分配直接复用该结果做高水位检查。
2. `pool_len` 等于初始化后不变的池容量；普通 alloc 用这一常量计算 free，不再每次调用
   `mem_stats()`。显式统计查询仍保持原实现。
3. 不改变 TLSF 算法、全局 mutex 临界区、关中断范围、布局、OOM 和非法释放策略；尤其不把
   dealloc 的统计更新移入 TLSF 锁，避免重现 ALLOC-01A 的严重退化。
4. 先跑 allocator 定向测试和双架构 Final check/build，再以 RISC-V 16 GiB/8 vCPU、
   `-snapshot` 完整 BuildStorm 对照 900.64 s；无稳定改善即回退代码。

恢复上下文命令：

```bash
codegraph explore "InterruptSafeTlsfHeap GlobalAlloc alloc dealloc realloc estimate_add mem_stats maybe_warn_high_water with_allocator_interrupt_guard exact source and all callers"
```

### 验证结果与结论

- allocator crate 裸 `cargo test` 因未选择架构 feature，在既有 `wateros-platform-arch` 中缺少
  `ArchTimeImpl/ArchInterruptImpl/ArchPagingImpl` 而无法独立链接；失败发生在改动 crate 编译前。
- `make check ARCH={rv,la} PROFILE=final` 与双架构 Final build：全部通过。
- RISC-V 完整快照 BuildStorm 连续两轮均成功，无 panic/SIGSEGV：
  - 首轮：`ok=true elapsed_s=895.78`，较 900.64 s 快 4.86 s（0.54%）；
  - 复核：`ok=true elapsed_s=901.13`，较基线慢 0.49 s；
  - 两轮均值 898.46 s，仅快 2.18 s（0.24%）。
- 最慢一轮发生退化，差异未越过运行噪声，因而代码全部回退，仅保留记录。日志为
  `/tmp/wateros-alloc01b-after-rv.log` 和 `/tmp/wateros-alloc01b-confirm-rv.log`。
- 结论：普通分配前的两个统计 load 不是可独立兑现的主要成本。下一步 allocator 工作应先
  获取真实尺寸分布/锁等待数据，再决定是否值得建设带旁路元数据的 per-CPU 小对象 cache；
  不应继续做原子指令级微调。

## MM-03A：页表层级跳过 mmap 候选空洞

状态：完整测试退化并回退（2026-08-10）

### 模块与热链

```text
sys_mmap / mremap
  -> find_free_mmap_base_considering_vmas
     -> 排除 stack/kernel-reserved/lazy/shared VMA
     -> for every candidate page
        -> translate_addr
           -> walk_find: root -> middle -> leaf
```

pc-hot 中 `find_free_mmap` 约 3.85 亿条指令。候选区间在排除有序 VMA 后，当前仍对每页从
根页表重走三级；上级目录项为空时，本可一次证明其覆盖的整段地址均未映射。

### 设计与保留门槛

1. 在 Sv39 与 LoongArch 页表实现中分别增加范围内首个已映射 VPN 的层级查询。
2. 无效中间 PTE 一次跳过该目录项覆盖范围；有效子表只扫描与查询相交的索引；叶 PTE
   返回范围内首个映射页。支持现有大页判断，即使用户映射目前只允许 4 KiB 叶。
3. `find_free_mmap` 保留 VMA/栈/保留区检查、冲突后跳一页、搜索上限和错误语义，只替换
   最后的逐页 `translate_addr` 循环；不增加需要在 munmap/fork/exec 同步的第二真相源。
4. 添加层级空洞/叶冲突定向测试，双架构 Final check/build 后以线上 RISC-V 配置和
   `-snapshot` 跑完整 BuildStorm；以 900.64 s 为保留基线。

恢复上下文命令：

```bash
codegraph explore "find_free_mmap_base_considering_vmas first_mapped_vpn_in_range walk_find vpn_indexes table_mut Sv39Pte LoongArch64Pte mmap_anonymous mmap_file_lazy mremap exact source and tests"
```

### 验证结果与结论

- 双架构地址空间自检增加了范围内叶映射定位、排除和 unmap 后为空的断言；双架构 Final
  check/build 均通过。
- RISC-V 完整 BuildStorm 生成了
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=905.58`，较 900.64 s 慢 4.94 s
  （+0.55%），无 panic/SIGSEGV。
- 测试组已打印 END，但父 bringup 数分钟未打印 command-succeeded/退出，出现异常尾部停滞；
  取得结果字段后手动终止 QEMU。代码全部回退并恢复双架构 Final。
- 结论：递归层级扫描虽然能跳过大空洞，但常见候选范围很可能较小，递归边界计算和函数调用
  抵消了省下的上级 walk；旧 pc-hot 的 3.85 亿指令不能直接等同于可兑现的墙钟收益。
  若再处理此链，应先取得 mmap 长度/页数分布，并考虑为 `walk_find` 返回缺失层级后在原循环
  原地跳跃，避免通用递归扫描器。
- 日志：`/tmp/wateros-mm03a-after-rv.log`（本机临时文件，不提交）。

## FS-04A：ext4 inode snapshot 写时复制缓存

状态：已实现并保留（2026-08-10）

### 证据与调用链

有效基线 Final 重跑 300 s `pc-hot fast=1`，`memcpy` 本体约 70.14 亿条指令。将所有直达
`memcpy` 的 callsite PC 与采样表、Final ELF 符号表关联后，主要调用者为：

- `Ext4::dir_find_entry`：19,028,354 次（FS-02A 已证明原地解析反而退化，不重复）；
- `copy_from_user`：4,949,253 次；
- `Ext4::read_inode`：3,051,436 次；
- `normalize_absolute_path`：2,167,418 次；
- `CachingBlockDevice::read_blocks`：1,804,969 次。

本项选择尚未尝试的 inode 链：

```text
lookup / getattr / open / read / write
  -> Ext4::read_inode
     -> inode_disk_pos
     -> block_cache.read_block
     -> Block::read_offset_as<Inode> memcpy
     -> Box<Inode> allocation
```

### 设计与保留门槛

1. `InodeRef` 内部改用 `Arc<Inode>` 的 COW 包装，保持现有 Deref/DerefMut 调用形式；只读
   inode snapshot 命中只增减引用，首次修改通过 `Arc::make_mut` 获得独立副本。
2. `Ext4` 增加固定 4096 槽的 direct-mapped inode snapshot cache；inode id 决定槽位，
   冲突覆盖。cache miss 保持原读盘/解码路径。
3. 所有 inode 回写最终集中到 `write_inode_without_csum`；成功写入 block cache 后同步发布
   新 snapshot。缓存不是写回真相源，不改变 checksum、flush、unlink 或 truncate 语义。
4. 定向测试覆盖重复读取命中、两个 reader 的 COW 隔离、写回后新 reader 可见；双架构
   Final check/build 后以 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 对照 900.64 s。

恢复上下文命令：

```bash
codegraph explore "another_ext4 Ext4 read_inode write_inode_with_csum write_inode_without_csum InodeRef Inode BlockCache load alloc_inode truncate generic_remove all callers exact source and tests"
```

### 验证结果

- `another_ext4` 定向测试：3/3 通过；新增测试证明两个 reader 初始共享 snapshot，writer
  首次修改后 COW 分离，原 reader 内容保持不变。
- `make check ARCH={rv,la} PROFILE=final` 与双架构 Final build：全部通过。
- RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=880.44`；测试完整结束，无
  panic/SIGSEGV，尾部正常退出。
- 相对 900.64 s 有效基线减少 20.20 s（2.24%）；相对 Linux 395.90 s 为 2.22 倍，
  距离 2 倍阶段门槛 791.80 s 尚差 88.64 s。
- 改后相同 300 s pc-hot：
  - `read_inode -> memcpy` callsite 从 3,051,436 降至 115,929（-96.20%）；
  - compiler `memcpy` 从 7,014,429,050 降至 6,483,942,336（-7.56%）；
  - TLSF allocate 从 2,034,503,114 降至 1,782,914,830（-12.37%）；
  - TLSF deallocate 从 1,443,820,979 降至 1,263,021,142（-12.52%）；
  - inode-cache mutex/Arc 未进入前 80 热点。
- 完整日志：`/tmp/wateros-fs04a-after-rv.log`；改前/改后采样：
  `/tmp/wateros-mem02-rv-pcs.txt`、`/tmp/wateros-fs04a-rv-pcs.txt`（均为本机临时文件）。

### 结论

固定容量 snapshot cache 同时消除了重复 inode 解码复制、Box/TLSF 分配和多数 block-cache
查询，收益显著且与 pc-hot 变化一致，因此保留。后续文件优化应继续优先减少“读取结构体即
分配拥有副本”的模式，而不是只改 memcpy 实现。

## MEM-04A：用户路径按页批量读取

状态：实验完成，代码已回退（2026-08-10）

### 证据与调用链

FS-04A 后的 300 s pc-hot 中，`copy_from_user` 仍约 7.9 亿条指令；其直接
`memcpy` callsite 在改前样本有 4,949,253 次。CodeGraph 展开后发现通用 MM 拷贝已经按
页处理，但 syscall 路径字符串辅助函数反而逐字节调用它：

```text
openat / newfstatat / execve / unlinkat / renameat / ...
  -> copy_user_path_cstr
     -> for each byte: ActiveUserMemoryOps::copy_from_user(1 byte)
        -> {Sv39,LoongArch64}UserMemoryOps::copy_from_user
        -> with_user_aspace_mut（每字节重新取得地址空间锁）
        -> translate_addr_with_perm（每字节重新走页表）
        -> copy_from_slice(1 byte)
```

### 设计、语义边界与保留门槛

1. 不修改 `UserMemoryOps` 稳定接口和双架构 MM 实现；只把 `copy_user_path_cstr` 改为每次
   读取“当前用户页剩余字节数”和 `max` 剩余长度的较小者，再在内核缓冲区中扫描 NUL。
2. 找到 NUL 后立即返回，不读取下一页；因此终止符后的未映射页面仍不触发 EFAULT。单页
   是当前页表权限与映射的最小粒度，读取终止符所在页的剩余已映射字节不会扩大跨页 fault
   边界。
3. 使用 `checked_add` 验证地址推进，保留空指针、`max == 0`、无 NUL 时
   `ENAMETOOLONG`、无效 UTF-8 时 `EINVAL` 和页访问失败时 `EFAULT` 的现有语义。
4. 定向测试覆盖页内 NUL、跨页扫描长度以及最大长度无 NUL 的扫描决策；随后跑双架构
   Final check/build 和 RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm。只有相对
   880.44 s 有明确改善且完整退出才保留。

首轮实现直接读取当前页全部剩余范围，完整 BuildStorm 为 898.65 s，比基线慢 18.21 s。
该版本把常见几十字节路径放大为最多约 4 KiB 的 memcpy/扫描，抵消了页表遍历收益。因此
第二轮仍保持不跨页，但把单次窗口限制为 128 字节：常见路径通常一次完成，最坏额外读取
限制为 127 字节。最终保留与否以第二轮完整测试为准。

恢复上下文命令：

```bash
codegraph explore "copy_user_path_cstr copy_from_user ActiveUserMemoryOps user_copy copy_from_user_in_aspace with_user_aspace_mut translate_addr_with_perm openat newfstatat execve unlinkat renameat exact source callers tests"
```

### 验证结果与结论

- 首轮按整页剩余范围读取：双架构 Final check/build 通过；RISC-V 完整 BuildStorm 正常退出，
  `ok=true elapsed_s=898.65`，比 880.44 s 基线慢 18.21 s（+2.07%）。
- 第二轮限制为 128 字节窗口：RISC-V Final check/build 通过；完整 BuildStorm 正常退出，
  `ok=true elapsed_s=901.04`，比基线慢 20.60 s（+2.34%）。
- 两个版本均无 panic/SIGSEGV，功能语义通过完整比赛镜像，但墙钟退化具有一致方向，排除
  “仅整页过读过大”这一单一原因。批量复制增加的内存流量、扫描和较长锁持有时间未被减少
  的页表 walk 抵消。
- 代码全部回退，有效基线仍为 880.44 s。本链后续若重做，应使用类似 Linux
  `strncpy_from_user` 的架构级 fault-safe word-at-a-time 读取，不能在 syscall 层先复制固定
  窗口；在 WaterOS 尚无 exception-table/fault fixup 基础设施时优先处理其他热点。
- 完整日志：`/tmp/wateros-mem04a-after-rv.log`、
  `/tmp/wateros-mem04a-v2-after-rv.log`（本机临时文件，不提交）。

## MEM-05A：消除 VFS 路由的 mount namespace 快照与重复路径分配

状态：实验完成，代码已回退（2026-08-10）

### 证据与调用链

对 FS-04A 后 300 s pc-hot 中所有直达 `__rust_alloc` 的指令按 callsite 计数并符号化，主要
来源包括：

- `normalize_absolute_path`：2,167,046 次；
- `String::clone` 内部分配：2,125,385 次；
- mount namespace / fs-bridge 路由相关分配：约 1,039,941 次；
- `resolve_material_route`：1,039,754 次；
- symlink resolver：单个 callsite 36–46 万次；
- ext4 `split_path` / RawVec 增长：约 34–62 万次。

CodeGraph 确认每次普通 VFS 路由都复制完整挂载命名空间，并为已经规范化的路径连续创建
两份拥有型字符串：

```text
lookup / metadata / open / read / write
  -> resolve_route
     -> mount_namespace_snapshot
        -> MountNamespace::clone
           -> Vec<MountEntry>::clone + mount String clones
     -> resolve_material_route
        -> normalize_absolute_path -> NormalizedPath(String)
        -> String::from(normalized.as_str())
```

### 设计、并发语义与保留门槛

1. `resolve_route` 改为通过现有 `with_current_namespace` 在 registry guard 下只读借用 namespace；
   `resolve_material_route` 仍返回完全拥有的 `FsRoute`，guard 在任何后端 FS 操作前释放。
2. 不修改 fork 的 mount namespace 深拷贝、`CLONE_NEWNS`、共享 namespace、mount/unmount
   更新语义。热路径只是从“锁内克隆后解锁并遍历副本”改为“锁内遍历原表并构造结果”。
3. 旧快照本就在 registry 锁内执行 Vec/String 分配；新路径不引入新的
   allocator-under-registry 锁序，且显著缩短锁内分配工作。`resolve_material_route` 不调用
   后端文件系统；返回后才进入 root/ext4/procfs。
4. `NormalizedPath` 增加消费式 `into_string()`，路由直接取得其唯一 String 缓冲区，避免
   `String::from(as_str())` 的第二次分配；保留现有 `as_str()` API。
5. 定向测试覆盖规范化结果消费、root/aux/bind 路由不变量；双架构 Final check/build 后，
   用 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 对照 880.44 s。明确改善才保留。

首轮同时启用“借用 namespace”和“转移规范化 String”后完整 BuildStorm 为 904.59 s，较
基线慢 24.15 s。功能正常且完整退出，但扩大 mount registry 临界区带来的串行化超过了
省下的快照克隆成本。因此第二轮恢复原有短临界区/namespace snapshot，只保留
`NormalizedPath::into_string()`，单独验证消除约 104 万次重复 String 分配的价值。

恢复上下文命令：

```bash
codegraph explore "normalize_absolute_path NormalizedPath resolve_route mount_namespace_snapshot with_current_namespace resolve_material_route MountNamespace MountEntry FsRoute rel_under_mount join_mount_path all callers exact source tests lock ordering"
```

### 验证结果与结论

- VFS API 定向测试 6/6 通过；fs-bridge 的裸 host test 因现有平台 crate 未选择
  `ArchPagingImpl` 无法独立链接，真实双架构 Final check/build 均通过。
- 首轮“借用 namespace + String 所有权转移”完整 BuildStorm 正常退出：
  `ok=true elapsed_s=904.59`，比 880.44 s 慢 24.15 s（+2.74%）。说明把 mount
  registry guard 延长到路径规范化/路由匹配会形成比 clone 更昂贵的串行化。
- 第二轮恢复 namespace snapshot，只保留 String 所有权转移：完整 BuildStorm 正常退出，
  `ok=true elapsed_s=883.96`，比基线慢 3.52 s（+0.40%）。减少一次分配没有兑现为可测
  墙钟收益，按“明确改善才保留”的门槛整体回退。
- 两轮均无 panic/SIGSEGV 或尾部停滞。有效基线仍为 880.44 s。
- 后续若优化 mount namespace，应考虑 Linux 风格的不可变/引用计数 mount tree 或按 generation
  缓存解析快照，而不是在单一全局锁下借用遍历；但在缺少 RCU/read-side 基础设施时优先处理
  其他无需扩大临界区的热点。
- 完整日志：`/tmp/wateros-mem05a-after-rv.log`、
  `/tmp/wateros-mem05a-v2-after-rv.log`（本机临时文件，不提交）。

## FS-06A：ext4 借用式路径分量遍历

状态：实验完成，代码已回退（2026-08-10）

### 证据与调用链

FS-04A 后 pc-hot 的直达 `__rust_alloc` callsite 显示，another_ext4 的路径拆分仍有多个热点：
RawVec 增长约 618,912 次，`split_path` iterator/collect 相关分配约 339,191 次，额外 Vec
构造约 49,916 次。CodeGraph 与当前源码确认：

```text
generic_lookup / generic_create
  -> split_path
     -> split('/').map(String::from).collect::<Vec<String>>()

generic_remove / generic_rename
  -> split_path -> Vec<String>
  -> split_off(last) + parent components join("/") -> String
  -> generic_lookup(parent String)
     -> split_path -> another Vec<String>
```

### 设计、生命周期语义与保留门槛

1. `split_path` 改为返回借用原始 `path` 的可克隆、双端 iterator；不生成 `Vec`，每个分量
   保持 `&str`。通过仅在去除前导 `/` 后的空路径上屏蔽 iterator，保留旧实现的根路径为空
   以及非根路径内部/尾部空分量形状。
2. `generic_lookup` 直接顺序遍历；`generic_create` 使用 `peekable()` 判断末分量，替代
   `enumerate + Vec::len`。
3. 新增借用型 `lookup_parent_and_name`：`next_back()` 取末分量，剩余 iterator 直接逐级
   lookup 父目录。remove/rename 不再 `split_off`、`join` 或再次拆分父路径。
4. iterator 与末分量引用都不存入 Ext4 或 inode，只在同步调用栈内使用；不改变目录项、
   inode、rename 原子性、错误码传播及磁盘格式。根路径用于 remove/rename 时由显式错误替代
   旧实现的下标 panic。
5. another_ext4 定向测试覆盖根、前导斜杠、内部/尾部空分量和双端遍历；双架构 Final
   check/build 后以 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm 对照 880.44 s。明确改善才
   保留。

首轮零分配 iterator 完整 BuildStorm 为 899.66 s，较基线慢 19.22 s。功能正常，但
iterator/filter/peek 的代码路径没有兑现分配下降。第二轮改用仓库 tmpfs 已采用的
`Vec<&str>`：保留连续切片、长度和索引的简单代码形状，只消除逐分量 String；同时继续
保留 parent 直接逐级 lookup，避免 join 和二次 split。

恢复上下文命令：

```bash
codegraph explore "os/vendor/another_ext4 Ext4 split_path generic_lookup generic_create generic_remove generic_rename lookup dir_find_entry rename exact source callers tests"
```

### 验证结果与结论

- another_ext4 定向测试 4/4 通过；首轮双架构 Final check/build 通过。
- 零分配借用 iterator 版本完整 BuildStorm 正常退出：
  `ok=true elapsed_s=899.66`，比 880.44 s 慢 19.22 s（+2.18%）。
- `Vec<&str>` 版本保留连续切片遍历，只去除逐分量 String，并继续避免 remove/rename
  parent join；定向测试和 RISC-V Final check/build 通过，完整 BuildStorm 正常退出：
  `ok=true elapsed_s=898.15`，比基线慢 17.71 s（+2.01%）。
- 两种实现均无 panic/SIGSEGV 或尾部停滞，但退化方向一致。说明此处 alloc callsite 次数高，
  实际成本却不是当前墙钟主导；旧实现拥有型连续数据的简单遍历/codegen 在 QEMU 下更有利。
- 代码整体回退，有效基线仍为 880.44 s。后续不再仅依据 allocator call count 选择目标，
  应优先选 pc-hot 指令占比和 wait-hot 阻塞时间同时较高的链路。
- 完整日志：`/tmp/wateros-fs06a-after-rv.log`、
  `/tmp/wateros-fs06a-v2-after-rv.log`（本机临时文件，不提交）。

## BLK-01A：VirtIO block 使用 direct descriptors

状态：实验完成，代码已回退（2026-08-10）

### 证据与目标调用链

最新有效 pc-hot 中 `VirtQueue::add_notify_wait_pop` 累计约 27.20 亿条指令；其
`add_indirect` 为每个块请求分配并清零 descriptor 数组。直达调用点归因显示
`add_notify_wait_pop` 引起约 220,583 次 `__rust_alloc_zeroed`，随后还要执行对应释放。

```text
BlockDevice::read_blocks/write_blocks
  -> VirtIOBlk::read_blocks/write_blocks
     -> VirtQueue::add_notify_wait_pop
        -> VirtQueue::add
           -> add_indirect (当前：每请求分配/清零/释放)
           -> add_direct   (目标：使用队列预分配 descriptor)
```

### 设计、边界与保留门槛

1. 新增只供 VirtIO block 实现使用的 transport adapter，在 feature discovery 时屏蔽
   `RING_INDIRECT_DESC` bit 28；`virtio-drivers` 其余协商和默认构造行为不变。
2. RISC-V MMIO 和 LoongArch PCI block 均使用该 adapter；网络、显示、输入设备不变。
3. 保持当前同步轮询、块缓存和文件系统语义。队列大小 16，一个 block 请求使用三个
   descriptor；当前单请求在途模型不存在容量不足。
4. 双架构 Final check/build 后，以 RISC-V 16 GiB/8 vCPU、`-snapshot` 完整 BuildStorm
   对照有效基线 880.44 s。pc-hot 必须确认 `add_indirect` 和其零分配调用消失；明确退化则
   回退代码，只提交实验记录。

恢复上下文命令：

```bash
codegraph explore "VirtIOBlk new feature negotiation Transport read_device_features VirtQueue add add_direct add_indirect MMIO PCI block initialization exact source callers"
```

### 验证结果与结论

- helper 单元测试 1/1 通过；四组 `ARCH={rv,la} PROFILE={pre,final}` check 及双架构
  Final 构建通过，只有仓库既有 warning。RISC-V direct 版本能正常识别块设备、挂载 Ext4、
  通过 VFS 自测，并完整跑完 CAgent 与 BuildStorm，无 panic/SIGSEGV。
- 首轮宿主 1200 s 保护超时前尚未完成；确认轮放宽保护时间后完整成功：
  `ok=true elapsed_s=896.66`，比有效基线 880.44 s 慢 16.22 s（+1.84%），超过既有约
  10 s 波动范围。
- 300 s pc-hot 确认 `add_indirect` 和其每请求零分配已从产物及动态调用中消失；
  `add_notify_wait_pop` 约 26.54 亿条指令，较旧样本约 27.20 亿略降，但 TLSF 总热点几乎
  不变，减少约 22 万次小分配不足以影响整体 allocator 成本。
- 结论：direct descriptor 在当前同步单请求和 QEMU TCG 下没有兑现墙钟收益，反而稳定
  退化。全部实现代码回退，仅保留本实验记录；有效基线仍为 880.44 s。后续不应把 direct
  descriptor 作为中断基础设施前置条件：异步设计可继续保留 indirect，待出现多请求队列
  深度和 descriptor 压力后再重新评估混合策略。
- 临时结果：`/tmp/wateros-blk01a-after-rv.log`、
  `/tmp/wateros-blk01a-confirm-rv.log`、`/tmp/wateros-blk01a-rv-pcs.txt`、
  `/tmp/wateros-blk01a-rv-top80.txt`（不提交）。

## IRQ-01A：固定容量设备 IRQ 注册表

状态：完成，等待平台控制器接线（2026-08-10）

### 任务与设计

为 RISC-V PLIC 和 LoongArch EIOINTC/PCH-PIC 提供共同的设备中断分发目标，先不改变
任何 block I/O 等待语义。`driver-api` 新增固定 32 项 registry、IRQ 编号、handler
回调、共享 IRQ 分发、冻结和诊断计数；注册只允许发生在启动期，分发路径不分配、不阻塞、
不访问文件系统。

```text
platform claim(irq)
  -> driver_api::interrupt::dispatch(irq)
     -> matching handlers (shared IRQ)
  -> platform complete/eoi
```

恢复上下文命令：

```bash
codegraph explore "driver-api irq registry register_handler dispatch trap external interrupt PLIC EIOINTC PCH-PIC exact source"
```

### 验证结果

- registry 单元测试通过：注册 handler、正确分发、未注册 IRQ 记为 spurious。
- RISC-V Final `make check` 通过；当前变更尚未接入硬件 claim/complete，也未启用 block IRQ。
- 下一步接入双架构平台控制器后再独立提交；若控制器测试失败，回退平台接线，不回退
  已验证的 registry API。

## IRQ-01B：外部中断平台接线

状态：双架构代码接线完成；LoongArch 运行验证受缺少镜像阻塞（2026-08-10）

### 设计与实现

- RISC-V 使用 QEMU virt DTB 的 PLIC（`0x0c00_0000`），按 hart supervisor context
  初始化 priority/enable/threshold，并提供 claim/complete。
- 架构层新增 Supervisor External Interrupt enable/disable；trap handler 循环 claim，
  调用 IRQ registry，再 complete 后继续 claim。
- LoongArch 使用 QEMU virt DTB 对应的 EIOINTC IOCSR 地址（ENABLE 0x1600、ISR 0x1800、
  ROUTE 0x1c00）和 PCH-PIC（0x10000000），将 PCI INTx 16..19 路由至当前 CPU；CPU
  hardware interrupt 3 按 ESTAT/ECFG bit 5 解码。

### 当前验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。
- `make kernel-la-final` 通过；LoongArch QEMU smoke 暂缺 `sdcard-la.img`，未伪造结果。
- RISC-V `kernel-rv-final` 使用 `-snapshot` 启动 smoke 通过，出现 `[fs] init end` 和
  `all commands finished`，无 panic/SIGSEGV。
- 尚未提交：控制器代码需先完成一次提交前审阅；当前尚未注册 VirtIO handler，IRQ
  registry 仍只处理未来 block phase 的设备中断。

## IRQ-01：双架构设备中断基础设施

状态：实施中（2026-08-10）

### 任务与设计

1. driver API 增加固定容量 IRQ handler registry，支持共享 IRQ、启动期冻结和无分配分发。
2. RISC-V 按 DTB 初始化 PLIC supervisor context，并接入 SEIE 与 external trap。
3. LoongArch 按实际 QEMU DTB 初始化 CPUIC/EIOINTC/PCH-PIC 和 PCI INTx 路由。
4. 此阶段不改变 block I/O，双架构完整 Final 不得出现性能退化或中断风暴。

恢复上下文命令：

```bash
codegraph explore "driver IrqLine MachineDriver init_after_boot trap SupervisorExternal RISC-V sie PLIC LoongArch CPUIC EIOINTC PCH-PIC PCI interrupt-map exact source callers"
```

## IRQ-01C：启动回归与链路审计

状态：平台控制器暂不在启动阶段启用（2026-08-10）

线上参数的 RISC-V `-snapshot` 全量运行在约 11 分钟后停在 cagent 前置阶段；日志同时
记录了 `cpu=5` 的 `StorePageFault`，`fault_addr=0xc20b000`。该地址正是 PLIC
supervisor context（hart 5）区域，说明当前内核的设备映射尚未覆盖 PLIC context，直接
写 threshold/claim 会触发内核页故障。此前的 `PLIC_ENABLE_BASE` 也曾误写为
`0x200000`，已修正为 QEMU virt 的 `0x2000`；但在 MMIO 映射和 VirtIO handler 完成
前仍不能打开外部线。

因此保留 registry、trap 分支和控制器代码供下一阶段使用，启动路径暂时只冻结 registry，
不调用 `init_external_irq` 或 `enable_external_interrupt`，先恢复同步 block 基线。下一步
必须先在平台 MMIO 映射层增加 PLIC/EIOINTC/PCH-PIC 区域并做无设备 handler 的 claim/
complete 冒烟，再进入单请求中断等待；不能把“代码可编译”当作控制器已可运行。
