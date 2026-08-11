# mprotect 按实际 PTE 变化执行 TLB flush/shootdown

## 为什么选择这里

新 syscall profiler 的 300 秒 BuildStorm 画像捕获 `340,830` 次 syscall，其中：

- `mprotect`：`100,154` 次，占 29.39%；
- 4 KiB 长度桶：`85,544` 次；
- `PROT_READ|PROT_WRITE`：`98,643` 次；
- 同窗口有 302 次约 128 MiB 的 mmap。

WaterOS 的 private anonymous mmap 已用 `ZeroAnonLoader` 登记为 lazy VMA。该调用形态符合
allocator 先保留大地址区、再逐页 `mprotect(RW)` commit 的模式。对于尚未驻留的 lazy
页，mprotect 只需更新 VMA 权限；当前实现仍在 mm-impl 末尾执行一次全地址空间 fence，
随后 `with_user_aspace_mut_and_flush` 又无条件本地全 flush 并请求所有缓存过该地址空间的
远端 CPU shootdown。

历史 MM-02A 曾尝试统一删除 brk/munmap/mprotect/madvise 的重复 flush，旧完整轮从
1023.91s 到 1033.79s（+0.97%）并回退。该结果没有 mprotect 参数/驻留分布证据，改动混合
多条 syscall，且当前已证明宿主存在约 20 秒漂移。因此只将其视为存疑证据，本实验不复刻
“无条件删一次 flush”，而是让操作返回实际 PTE 变化摘要。

## 选择的方案

1. `MmapOps::mprotect` 返回 `MmResult<bool>`：`true` 表示至少一个驻留叶 PTE/PPN 发生
   变化，`false` 表示只更新 lazy VMA 元数据或权限本就相同。
2. 遍历每页时读取 `leaf_page_perm`：
   - 未驻留且属于 lazy VMA：跳过 PTE 更新，不记 changed；
   - 已驻留且权限相同：不重复写 PTE；
   - COW/private-write 或权限不同：执行原有语义并记 changed。
3. mm-impl 不再无条件执行 `fence_user_ptes()`；新增
   `with_user_aspace_mut_and_flush_if_changed`，仅在闭包成功返回 changed 时执行一次本地
   full flush 和远端 shootdown。
4. syscall mprotect 使用该 helper。成功且 changed=false 时不刷新；成功且 changed=true 时
   刷新。错误路径无条件保留原有 full flush + shootdown，因为范围后部报错前可能已修改前部
   PTE，不能从 `Err` 中取得变化摘要。
5. RISC-V/Sv39 与 LoongArch64 保持对称；不改变 VMA 拆分、COW、权限检查和 errno。

## 为什么这样做

- Linux 对未驻留 VMA 的 mprotect 主要修改 VMA 元数据，不为不存在的 PTE 做 TLB
  shootdown；本方案学习这一原则，但不引入 Linux 的 mmu_gather/rmap 复杂度。
- bool summary 把“是否修改页表”从猜测变成接口事实，避免再次无条件删除同步。
- 目标覆盖约 10 万次调用，比继续优化单个 lookup/memcmp 更可能产生架构级收益。
- 已驻留页继续使用现有 SMP shootdown，一致性风险集中且可测试。

## 实现与验证步骤

1. 修改 MmapOps 合约、双架构 mprotect 和双架构 user-aspace conditional helper。
2. 定向测试至少覆盖：
   - 未驻留 lazy anon 仅改 VMA并返回 false；
   - 已驻留页权限改变返回 true；
   - 相同权限返回 false；
   - PROT_NONE/R/RW 与 COW 写权限语义不变。
3. 运行受影响 crate 测试、RISC-V/LoongArch Final check 与 kernel build。
4. 用 syscall profile 确认调用形态不变；用 pc-hot/wait-hot 或临时计数确认 conditional
   helper 的 no-flush 分支命中率，而不是只看 mprotect 调用数。
5. 固定镜像、CPU `0-15`、`TMPDIR=/tmp`、`-snapshot` 交错运行完整 BuildStorm。

## 验收与回退门槛

- 功能、双架构静态门禁和定向权限测试全部通过。
- 完整候选至少两轮；与相邻 main 交错对照，改善可复现且至少 1.5%。本项以大收益为目标，
  若只落在噪声内不合并。
- 出现权限泄漏、COW 破坏、远端 stale TLB 或完整轮不稳定，立即回退代码并保留文档。

## 实现结果

实现保持了方案中的双层变化摘要：

- 双架构 `MmapOps::mprotect` 只在驻留 PTE 权限或 PPN 实际改变时返回 `true`；lazy
  VMA-only 和相同权限路径返回 `false`；
- syscall 层改用 `with_user_aspace_mut_and_flush_if_changed`；成功且未改 PTE 时跳过本地
  full flush 与远端 shootdown，成功且 changed=true 时只执行一轮；
- 错误路径仍无条件刷新，覆盖 mprotect 范围前部已改、后部发现未映射页的部分修改情况；
- 定向自检覆盖驻留权限变化、驻留相同权限和 lazy VMA-only 三条路径，并在 lazy
  mprotect 后触发写缺页确认最终 PTE 为 `R|W|U`。

静态门禁与构建结果：

- `make rv_check`：通过；
- `make la_check`：通过；
- `make kernel-rv-final`：通过；
- 候选内核 SHA-256：
  `ea091ca43109ad0ae13b2da3ce14acb2504d60cc1e8ea8f0a6b49441450082d2`。

## 固定镜像完整 A/B

固定条件：镜像 SHA-256
`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`，宿主 CPU
`0-15`，`TMPDIR=/tmp`，QEMU `-snapshot`。交错序列：

| 顺序 | 内核 | guest elapsed | 结果文件 |
| --- | --- | ---: | --- |
| 1 | candidate A1 | 810.26s | `/tmp/wateros-buildstorm-fixed/fixed-mprotect-full-a1/result.json` |
| 2 | fixed main A4 | 829.67s | `/tmp/wateros-buildstorm-fixed/fixed-main-full-a4/result.json` |
| 3 | candidate A2 | 803.36s | `/tmp/wateros-buildstorm-fixed/fixed-mprotect-full-a2/result.json` |

候选中位数为 `806.81s`。相对夹在两轮候选之间的 main A4 改善 `22.86s`，即
`2.76%`；两轮候选分别比 main A4 快 `2.34%` 与 `3.17%`。所有轮次的 toolchain、
minibuild、multi compile 标记均通过，无 timeout/panic/SIGSEGV。

该结果超过 1.5% 合入门槛，但仍远小于 WaterOS 与 Linux RISC-V baseline
（约 `395.90s`）的差距，因此将其分类为“可合入的中等收益同步优化”，而不是主突破口。
下一项应转向重复 ELF/只读文件页共享，消除跨进程重复分配、清零和复制。

## Hot 诊断

使用同一镜像、CPU 亲和性和 300 秒窗口运行候选 `pc-hot`：

- 结果：`/tmp/wateros-buildstorm-fixed/fixed-mprotect-pchot-a1/result.json`；
- PC 数据：`/tmp/wateros-buildstorm-fixed/fixed-mprotect-pchot-a1/pc-hot.txt`；
- 固定窗口按预期在完整 compile marker 前 timeout，无 panic/SIGSEGV；该轮只作诊断，
  不进入墙钟验收；
- fixed main：`29,847,188,341` 条 guest 指令；候选：`32,059,654,127` 条，候选在同一
  300 秒窗口推进的 guest 指令数多 `7.41%`；
- `sys_mprotect + MmapOps::mprotect` 占比从约 `0.199%` 到 `0.208%`，没有形成新热点；
- `request_tlb_shootdown_targets` 占比从约 `0.0185%` 降到 `0.0162%`。由于同窗口候选推进
  的工作更多，原始调用/指令数不能直接当作节省量，但归一化结果未显示成本转移。

完整 A/B 与 hot 诊断方向一致：条件同步优化有效，可合入 main。
