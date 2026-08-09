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
