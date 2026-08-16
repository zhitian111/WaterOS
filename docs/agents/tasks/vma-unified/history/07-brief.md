# Task 07 简报：VMA 最终功能/性能验收完成

## 完成情况

VMA 统一路径已完成双架构最终 BuildStorm 验收。RV 单核/8核、LA 单核/12核
均通过，且 LA 12 最终性能命令使用用户给定的 `-m 36G` 配置。

## 功能/性能结果

| 架构 | 配置 | BuildStorm 结果 | elapsed_s | 日志 |
|:--|:--|:--|:--|:--|
| RV | 单核 | `status=OK run=OK` | 1329.63 | `/tmp/wateros-vma-rv-single.log` |
| RV | 8 核 | `status=OK run=OK` | 547.65 | `/tmp/wateros-vma-rv-smp8-clean.log` |
| LA | 单核 | `status=OK run=OK` | 1246.33 | `/tmp/wateros-vma-la-single.log` |
| LA | 12 核 | `status=OK run=OK` | 513.19 | `/tmp/wateros-vma-la-smp12-final36g.log` |

日志 SHA-256：

```text
273513899163af18805e8a68bedbae6debccd7ab62cd54f119a4694bcbd58b7a  /tmp/wateros-vma-rv-single.log
8299c6592338b8801c309e97adc735a8c0725091f71ef34e34947d4e272c98ed  /tmp/wateros-vma-rv-smp8-clean.log
744fc1da9eca9a86d29fb7162ff6f3b09dd9e5dccb939d76486f884e198daa39  /tmp/wateros-vma-la-single.log
fd5749a1d4df65dabd7d76e1fa0628bff906bd659ba8b65e856f1f08e540f374  /tmp/wateros-vma-la-smp12-final36g.log
```

## 静态验收

```text
make rv_check          PASS
make la_check          PASS
make kernel-rv-final   PASS
make kernel-la-final   PASS
git diff --check       PASS
```

## 结论

- VMA 分支当前功能路径通过双架构 SMP 验证；
- LA 12 最终性能结果 `elapsed_s=513.19`，`run=OK`；
- 后续可进入 slab 分支 rebase 交接阶段。

## 待办交接

- 在 slab 工作树 `perf/kernel-heap-slab` 上 rebase 到当前
  `refactor/vma-unified` 分支 HEAD；
- 更新 `docs/agents/tasks/kernel-heap-slab/RECOVERY-REBASE.md`；
- 按 slab 任务文档重新执行 RV/LA BuildStorm 性能验证。
