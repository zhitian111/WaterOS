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
