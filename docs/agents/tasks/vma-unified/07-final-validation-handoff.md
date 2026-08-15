# Task 07：最终功能/性能验收与 slab rebase 交接

## 任务目标

完成 VMA 分支最终验收，准备与 `perf/kernel-heap-slab` 分支 rebase 的交接条件。

## 当前进度

- 静态验收已通过：`make rv_check`、`make la_check`、`make kernel-rv-final`、
  `make kernel-la-final`、`git diff --check`。
- RV 单核、RV 8 核、LA 单核完整 BuildStorm 已通过。
- LA 12 核当前受宿主机内存压力阻塞，详见
  `07-LA12-RESOURCE-BLOCKER.md`。
- 在 LA 12 通过前不生成最终 `history/07-brief.md`，也不标记本任务完成。

## 实施方案

1. 功能验收：

   - RV 单核/8 核、LA 单核/12 核完整 BuildStorm；
   - `make rv_check` / `make la_check`；
   - `git diff --check`。

2. 性能验收：

   - `elf-lazy-map` 已恢复默认开启；
   - RV/LA BuildStorm 与 main 基线比较，无明显退化；
   - 保存日志 SHA 和 `elapsed_s`。

3. 交接文档：

   - 写入 VMA 分支当前 HEAD、关键提交；
   - 说明 slab 分支 rebase 到本分支的冲突点和顺序；
   - 列出后续需要 slab 分支重新验证的命令。

## 涉及文件

- `docs/agents/tasks/vma-unified/history/07-brief.md`
- `docs/agents/tasks/kernel-heap-slab/RECOVERY-REBASE.md`（在 slab 分支更新）
- VMA 相关源码

## 验收方式

```bash
cd /tmp/wateros-vma-unified/os
make rv_check
make la_check
git diff --check
```

运行时结果记录在 brief 中。

## 完成后

新增 `history/07-brief.md`，并确认 slab 分支的恢复文档已更新。
