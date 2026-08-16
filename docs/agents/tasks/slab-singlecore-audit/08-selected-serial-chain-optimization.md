# Task 08：实施一个有证据的串行编译链路优化

## 任务目标

只针对任务 07 证明的最大内核占比链路实施一个独立优化提交。不得把“某个 crate 编译时间
长”直接当作 VFS 或 allocator 根因。

## 实施方案

任务 07 结束后在本文件的 history 中锁定候选类别和具体符号，再修改对应语义所有层：

- mmap/mprotect/page fault：只改 MM/VMA/TLB 的实际热路径，保持 COW、权限和 shootdown 语义；
- metadata/path：只改 VFS/FS bridge 的重复锁/复制，必须有命中与失效证据；
- file read/block I/O：只改 page-cache/block-cache/virtio 的确认瓶颈，保留 sync/writeback 语义；
- scheduler/futex：只改等待/唤醒或迁移链路，证明无丢唤醒；
- process/exec：只在长尾窗口显示 fork/exec 占比时修改生命周期代码。

不允许预先重做已失败的简单 dcache、增大 readahead、ELF prefix 或仅减少 snapshot 构造。
若任务 07 无法区分内核和用户态成本，本任务改为提交诊断结论，不提交生产优化代码。

## 验收方式

```bash
cd os
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

按任务 00 的镜像流程做功能 smoke 和 RISC-V A/B/B/A；至少两轮完整 BuildStorm。必须满足：

- 无 panic/OOM/ENOMEM/SIGSEGV/fault、缓存失效、丢唤醒或产物错误；
- 目标窗口耗时和相应 pc/syscall 指标方向一致；
- 完整轮相对任务 07 前候选至少改善 1.5%，否则回退本任务并保留失败简报。

## 涉及文件与 CodeGraph

由任务 07 选定后填写精确文件清单；开始代码修改前必须执行：

```bash
codegraph explore "<任务07选定符号> exact source callers callees"
codegraph impact "<任务07选定公共API或锁>"
```

## 完成后

新增 `history/08-brief.md`，记录根因、修改层、功能门禁、目标窗口和完整 A/B 结果。
