# VMA 统一路径重构任务

本目录用于从当前 `main` 独立推进 VMA 路径统一，避免与 `perf/kernel-heap-slab`
分支相互污染。目标是把 Sv39 / LoongArch64 两套高度重复的 VMA 数据结构与
split/merge/mprotect/mremap 逻辑收口到共享层，先修复 lazy VMA 有序性不变量，
再逐步向 Linux 的 VMA + page cache 思路靠拢。

## 分支与工作树

- 分支：`refactor/vma-unified`
- 工作树：`/tmp/wateros-vma-unified`
- 起点：当前 `main`
- CodeGraph：已初始化于 `/tmp/wateros-vma-unified/.codegraph`

## 目标

1. 双架构共享 VMA 表示和修改路径；
2. 所有 VMA split/merge/remove/protect/mremap 都经过统一入口；
3. 保证 `lazy_file_vmas` 始终有序、无重叠；
4. 重新打开 Sv39 `elf-lazy-map` 并完成双架构功能/性能回归；
5. 逐步把 loader 从 VMA 中解耦，缺页统一走文件 backing/page cache。

## 验收主线

### 功能

- `make rv_check` / `make la_check` 通过；
- RV 单核与 8 核、LA 单核与 12 核完整 BuildStorm 无 SIGSEGV/panic/OOM；
- 现有 mm 自检和相关 syscall 回归通过。

### 性能

- 重新开启 `elf-lazy-map` 后，RV/LA BuildStorm 不出现明显性能退化；
- 后续 slab 分支 rebase 后继续做纯 slab A/B。

## 任务顺序

| 任务 | 目标 |
|---|---|
| `00-vma-inventory.md` | 盘点双架构 VMA 结构、调用路径与有序性不变量 |
| `01-common-vma-types.md` | 把 VMA 类型与基础方法抽到 `mm-impl/common` |
| `02-ordered-vma-registry.md` | 建立有序、无重叠的 VMA 注册表/索引 |
| `03-unified-split-merge-ops.md` | 统一 split/merge/protect/remove/mremap 操作 |
| `04-reenable-lazy-elf-validate.md` | 重新打开 `elf-lazy-map` 并做双架构完整回归 |
| `05-decouple-loader.md` | 将 `Box<dyn DemandPageLoader>` 从 VMA 中解耦 |
| `06-page-cache-fault.md` | 文件缺页统一走 page cache/backing 路径 |
| `07-final-validation-handoff.md` | 最终功能/性能验收与 slab rebase 交接 |

## 每个任务完成后

新增 `docs/agents/tasks/vma-unified/history/<task-id>-brief.md`，记录：

1. 完成情况；
2. 改动文件与关键 diff；
3. 实际验收命令与结果；
4. 未验证项和下一任务前置条件；
5. 文档同步情况。

## 提交约定

- 一次任务一个 commit；
- 提交信息：`[vma] <task-id> <一句话说明>`；
- 提交前 `git diff --check`；
- 不提交 `kernel-*`、`*.img`、`target/`、日志和 `.codegraph/`。
