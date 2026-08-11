# BuildStorm 重复 ELF 只读页装载画像

## 为什么选择这里

syscall 画像在 300 秒窗口内记录到 233 次 `execve`，路径重复率为 62.23%；`cargo`、
`rustc`、linker、动态解释器和 libc 等 ELF 会被不同编译进程反复启动。当前
`ElfPathSegmentLoader::load_page` 的每次 lazy fault 都会：

1. 分配一个新 4 KiB 物理帧；
2. 清零整页；
3. 通过同一路径读取该 ELF 区间并复制到新帧；
4. 将只属于当前地址空间的 PPN 写入页表。

VFS 已有文件页缓存，因此底层块读取可能命中，但新帧分配、清零和从 VFS cache 到用户
帧的复制仍会跨进程重复。Linux page cache 会让只读干净文件页直接被多个地址空间映射；
WaterOS 帧分配器也已经支持 `frame_inc_ref`/`frame_dealloc` 引用计数，具备学习这一机制的
基础。

不过，233 次 exec 不能直接推出可共享页数：进程可能只触发少量页，也可能触发数千页。
在实现缓存前，必须先量化页级重复率与可避免复制字节数。

## 选择的分析方案

在独立诊断分支中为双架构 `ElfPathSegmentLoader` 增加临时、最终不会合入 main 的页键统计：

- 只统计 `!perm.writable()` 的 ELF lazy segment；可写数据/BSS 不进入候选集合；
- 每次 `load_page` 以 resolved path hash、ELF segment 布局和 loader file offset 组成内容键；
- 记录总 cacheable fault、唯一键、重复键以及 `重复键 × 4096` 的可避免复制字节上限；
- 每累计 8192 次只输出一条汇总，不输出逐页路径或 fault 日志；
- 画像结束后回退统计代码，只在本文档保留结果和结构化运行文件路径。

键包含段布局是因为 ELF `PT_LOAD` 可能页不对齐，页内容由文件字节、前后零填充和段边界
共同决定；仅使用 `(path, file_offset)` 可能错误共享内容不同的边界页。

## 为什么先画像而不直接实现

- 之前 ext4 lookup cache 已证明“重复访问很多”不等于“缓存一定改善端到端”；
- VFS page cache 已经消除一部分设备 I/O，剩余收益主要是帧分配、清零和复制，必须确认
  重复页规模足够大；
- 精确命中上限可用于决定第一版缓存容量，避免无界缓存占用大量物理内存；
- 画像还能确认只读页覆盖率，避免为了少量页改动 DemandPageLoader/MM API。

## 接下来的工作

1. 实现临时页键聚合与稀疏汇总，完成 RISC-V/LoongArch check 和 RISC-V Final build。
2. 固定镜像、CPU `0-15`、`TMPDIR=/tmp`、`-snapshot` 运行 300 秒 BuildStorm 画像。
3. 只读取汇总行和 runner `result.json`，计算重复率、唯一工作集和可避免复制量。
4. 若重复页比例至少 30%，且 300 秒可避免复制至少 128 MiB，则进入独立实现分支：
   - 有界只读 ELF 页缓存；
   - 缓存持有一个 PPN 引用，映射命中再 `frame_inc_ref`；
   - 只读 PTE 直接共享，可写段保持原路径；
   - 缓存键/失效语义必须防止文件内容变化后复用陈旧页。
5. 若未达到门槛，记录为低收益方向并转向路径元数据或调度等待链路。

## 验证与验收

- 诊断本身不作为性能样本，不能用它的墙钟时间判定优化；
- 画像不得改变页内容、权限、PPN 或地址空间生命周期；
- 后续实现必须先写新的优化方案文档，并完成双架构门禁、定向共享/引用计数测试、hot
  分析和固定镜像交错完整 A/B；
- 后续候选仍以可复现且至少 1.5% 为合入底线，并以 10% 级收益为目标。

## 结果

诊断内核通过：

- `make rv_check`：通过；
- `make la_check`：通过；
- `make kernel-rv-final`：通过；
- 内核 SHA-256：
  `cc226f629e98ca7a897d547642d460e456c7dca7d5f525117eb7d6a07131bec3`。

固定镜像 SHA-256
`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`，CPU `0-15`，
`TMPDIR=/tmp`，QEMU `-snapshot`，运行 300 秒：

- runner：`/tmp/wateros-buildstorm-fixed/fixed-elf-page-profile-a1/result.json`；
- serial：`/tmp/wateros-buildstorm-fixed/fixed-elf-page-profile-a1/serial.log`；
- 300 秒诊断窗口按预期 timeout，toolchain/minibuild 通过，无 panic/SIGSEGV；完整 compile
  marker 未到达，因此该轮不作为墙钟性能样本。

最后一个 8192 页汇总点：

| 指标 | 数值 |
| --- | ---: |
| cacheable readonly ELF faults | 40,960 |
| unique content keys | 7,325 |
| repeated loads | 33,635 |
| repeated ratio | 82.11% |
| avoidable zero/copy lower bound | 137,768,960 bytes（131.39 MiB） |
| unique readonly working set | 30,003,200 bytes（28.61 MiB） |

最后汇总点到 300 秒终止之间的页未计入表中，因此这里的总 fault 和可避免字节是下界；
重复率已经稳定从 71.76% 上升到 82.11%。结果同时超过“重复至少 30%”和“可避免复制至少
128 MiB”两项预设门槛。

## 决策

进入独立实现分支，开发有界只读 ELF PPN cache。第一版目标：

- cache 只接收 `!W` 的 `ElfPathSegmentLoader` 页；
- 以 resolved path、段布局和 file offset 作为内容身份；
- cache miss 只允许一个装载者填页，成功后 cache 持有一份 frame ref；
- cache hit 先 `frame_inc_ref`，再将同一 PPN 以只读权限映射进新地址空间；
- 容量以画像的约 7.3k 唯一页为依据，选择可覆盖工作集但有明确上限的容量；
- 处理文件修改失效，不能让同一路径的新 ELF 复用旧内容；
- 双架构实现、引用计数/地址空间销毁测试、pc-hot 和固定镜像交错完整 A/B 后再决定合入。
