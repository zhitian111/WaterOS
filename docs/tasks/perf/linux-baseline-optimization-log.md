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
