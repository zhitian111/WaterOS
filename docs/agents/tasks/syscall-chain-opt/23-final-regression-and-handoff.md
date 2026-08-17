# 任务 23：最终回归、文档同步与交接

## 任务内容与目标

对所有已保留 commit 做统一功能、性能、双架构和持久化验收，清理仅供实验的诊断开关，更新
受实现变化触发的权威文档，并形成可复现交接。本提交只做最终测试修正、文档和必要的小型
清理，不再引入新的优化机制。

## 实施方案

1. 审计每个 `history/*-brief.md`、提交 hash、回退结论和未验证项，未完成任务明确标记。
2. 运行双架构 check/build；RISC-V 用任务 00 QEMU 9.2.1 `-snapshot` 命令做最终 8 核完整轮。
3. 对路径、FD、exec/MM、FS、futex 分别跑定向回归；写回使用可丢弃副本重挂载/e2fsck。
4. 交错比较分支起点与最终候选，报告总墙钟、阶段时间、内存、fault、锁等待、flush 与 TLB。
5. 按 AGENTS 文档触发矩阵更新 README、组件契约、feature、脚本和工具文档；不改写历史报告。

## 涉及文件

- 本目录 `README.md`、所有任务简报和最终 `history/23-brief.md`
- 实际触发的 `README.md`、`os/README.md`、组件 README、`docs/tools/`、`docs/workflows/`
- 只允许必要的测试/诊断清理代码

## CodeGraph / 审计命令

```bash
codegraph sync .
codegraph affected $(git diff --name-only e54000d9..HEAD -- 'os/**/*.rs')
rg -n "TODO|temporary|syscall-chain-opt" os/components os/src
rg --files os/components -g 'implementation.rs'
```

## 验收方式

```bash
cd os
make configure
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
cd ..
git diff --check
git status --short
```

随后执行 README 的 QEMU 9.2.1 `-snapshot` 命令完成最终 RISC-V BuildStorm；LA 使用项目对应
固定 runner。确认无生成物、镜像、日志、`.codegraph` 或他人改动进入提交。

## Commit 与简报

提交建议：`[docs] 完成 syscall 链路优化回归与交接`。新增 `history/23-brief.md`，汇总最终
commit 序列、功能矩阵、A/B 表、剩余风险和推荐合并方式。
