# 全局预清零物理帧池设计

## 目标

将用户匿名页、零填充映射和页表页所需的 4 KiB 清零从繁忙的 fault CPU 转移到 idle CPU，
同时用批量 raw frame 分配减少全局 frame allocator 的锁竞争。该池是可回收缓存：不改变
普通 raw 分配返回“内容未定义”的契约，也不改变 dirty 页回收路径。

## 数据结构与所有权

池是一个全局固定数组实现的 LIFO 栈，而不是队列。LIFO 只需要尾部 push/pop，临界区只读写
PPN 和 `len`；取帧、清零和归还 allocator 都不在同一个临界区执行。

```text
allocator --alloc raw frame--> producer owns frame
                                   |
                              zero outside locks
                                   |
                                   v
                         global zeroed LIFO pool
                                   |
                              pop transfers ownership
                                   v
                         page fault / page table / mapping
```

- 从 allocator 取出后，frame 保持 `allocated=true, ref_count=1`。
- 发布进池前必须完成整页清零；池锁的释放/获取提供发布与观察顺序。
- pop 只转移这一个引用，不修改 allocator 元数据。
- 已被用户或内核写入后释放的帧回普通 allocator，绝不回流 zeroed 池。
- raw allocator OOM 时，池按小批量 drain 并归还 allocator 后重试，因此池不是永久预留。

`in_flight` 记录已由 producer 占用、但尚未完成清零并发布的槽位。它与 `len` 一起限制总数，
防止多个 idle CPU 并发补池时超过容量。

## 参数

当前正式参数：

```text
容量                    1024 页（4 MiB）
低水位                  256 页（1 MiB）
高水位                  1024 页
同步 miss 补充批量      16 页
idle 补充批量           32 页
OOM drain 批量          32 页
```

容量从 256 页提升到 1024 页，是因为 8 vCPU 的 BuildStorm 测量中 256 页多次满池，仍出现短时
demand miss。OOM drain 不再按池容量在内核栈上创建 PPN 数组；容量增大后仍只使用 32 个 PPN 的
临时数组，避免 8 KiB 栈对象。

## 运行流程

### demand allocation

1. 从全局池 pop；命中立即返回。
2. miss 时，当前 CPU 在短临界区预留最多 15 个发布槽位。
3. 它以一次 allocator 锁获取批量分配“返回页 + 预留页”。
4. 所有页都在 allocator/pool 锁外清零；返回第一张，其余页以一次短池锁发布。
5. allocator OOM 时，先从池中摘下最多 32 页，锁外归还 allocator，再重试。

### idle maintenance

idle loop 每次 WFI 前调用一次维护钩子：当有效存量（`len + in_flight`）低于低水位时，尝试以
`try_lock` 从 allocator 批量取得 raw frame。拿不到 allocator 锁时直接跳过本轮，不自旋、不唤醒
额外 worker。成功后在锁外清零，再短暂持池锁发布。这样 single-core 繁忙时，其他 idle CPU 可以
消耗空闲周期预制零页。

### ELF BSS

ELF loader 只对完整 pure-BSS 页尝试从池中直接建立映射；file-backed、混合边界页仍按原有 eager
填充处理。池低于保留水位或没有可用页时，loader 保留 lazy VMA，后续仍由 fault 路径完成，因此
该优化不会把内存不足变成加载失败。

## 并发与锁约束

1. 每个 PPN 只可能位于普通 allocator、zeroed 池、producer in-flight 或交付调用者之一。
2. 清零期间 frame 不可被映射或释放，producer 独占其唯一引用。
3. 不同时持有 allocator 锁和 zeroed-pool 锁。
4. 不在任一锁内清零、调度、用户拷贝或打印日志。
5. idle 路径不能因池维护自旋；allocator 忙时直接 WFI。

## 诊断策略

诊断计数由实现 crate 的 `zeroed-frame-pool-stats` feature 控制，默认关闭。正式性能内核不会维护
hit/miss、refill 或水位计数；bring-up 队列里的复位和打印调用也保留为注释。下次完整内核测量前，
需要沿 facade feature 链转发该 feature，再恢复对应两行，避免将探针本身混入正式性能结果。

## 验证

- `make rv_check`：通过。
- RISC-V final 内核：构建通过。
- QEMU 9.2.1、16 GiB、8 vCPU、snapshot 镜像 BuildStorm：256 页测量配置下通过，详见
  `2026-08-17-riscv-single-core-rustc-optimization-report.md`。

1024 页配置尚未进行端到端复测；其选择基于 256 页测量的峰值和 miss 数据，不应提前表述为已验证的
性能收益。
