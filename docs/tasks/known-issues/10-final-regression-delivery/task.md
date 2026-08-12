# K-10：最终回归、结果冻结与交付

## 任务目标

冻结候选 commit，在 RISC-V64 和 LoongArch64 上完成可复现的功能、性能、SMP 和文件
系统完整性验收；整理提交、镜像、日志和设计材料。此任务只接受阻断性修复，不再引入
新的性能策略。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/release-overview/current.md`
- `docs/prompts/tasks/run_testsuits_qemu.md`
- `docs/tasks/known-issues/README.md`
- `final_test_case/README.md`

## 前置条件

K-01 至 K-04 和 RIO-01..10 完成。K-05 至 K-09 中只有 K-04 选择且通过 A/B 门禁的
改动进入候选；其余任务可标为“未选择/后续”，不阻塞正确版本发布。

## 已知信息与代码证据

- 当前工作树包含正式修改、用户修改、未跟踪镜像/内核/日志，不能用一次
  `git add -A` 生成候选提交。
- 原始 final 镜像曾被直接可写启动；最终测试必须使用只读基线和独立 overlay。
- CAgent、BuildStorm 和测试脚本的外层退出码曾不能可靠反映内部失败，因此必须同时
  检查精确成功标记和命令退出码。

候选清单至少记录：

```text
kernel_commit=
user_submodule_commit=
image_sha256=
qemu_version=
firmware_version=
command=
log=
```

## 涉及文件

- `os/Makefile`
- `os/scripts/{rv,la}_{pre,final}_run.sh`
- `os/scripts/testing/run_phase_tests.sh`
- `os/src/user_bringup_busybox.rs`
- `final_test_case/README.md`
- `docs/prompts/tasks/run_testsuits_qemu.md`
- `docs/tasks/known-issues/`
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`
- `docs/tasks/history/known-issues/README.md`
- `.github/PULL_REQUEST_TEMPLATE.md`

## 任务内容

1. 记录主仓库和 `user` submodule commit；审核 `git status --short`，只提交任务相关
   源码、测试和文档。
2. 从干净 checkout 构建 RV/LA pre/final kernel，记录命令、大小和 SHA-256。
3. 每轮从同一只读 raw 镜像建立独立 qcow2 overlay；不得让两个 QEMU 写同一 overlay。
4. 两架构验证 8 CPU、CAgent 三轮、完整 BuildStorm、初赛 basic/busybox、LTP 关键
   集合和 read-family 差分测试。
5. 性能测试至少三轮，报告单轮值、中位数和离散程度；逐个关闭保留优化完成消融。
6. 每轮 FS 写 workload 后将 overlay 转 raw 并运行 `e2fsck -fn`；记录原始镜像前后
   hash。
7. 删除临时 trace/counter、测试专用 bringup 命令和生成物；保留默认关闭的结构化
   诊断 feature。
8. 最终文档写明已完成、未选择、已知限制和外部阻断。缺少某架构镜像/固件时不能用
   空表或另一架构结果代替。

## 如何验收

- [ ] `git diff --check` 通过，提交范围人工复核，无 kernel/image/overlay/大日志。
- [ ] 干净 checkout 可执行 `make rv_check && make la_check` 并构建四个目标。
- [ ] 两架构均 8 CPU online，timer、IPI、task 和 TLB shootdown 正常。
- [ ] CAgent 连续三轮 10/10。
- [ ] BuildStorm 输出 toolchain、minibuild、`COMPILE mode=multi ok=true`，产物合格。
- [ ] read-family、basic、busybox 和选定 LTP 无新增失败。
- [ ] 所有写测试 overlay 的 `e2fsck -fn` 五阶段通过，基线镜像 hash 不变。
- [ ] 性能结果可由日志重算，优化收益可通过单项消融复现。
- [ ] 正式日志无临时逐 syscall/page/packet 输出和未解释 warning。
- [ ] 最终报告中的每个结论都能追溯到 commit、命令、镜像和日志。

最终结果写入 `docs/tasks/history/known-issues/final-YYYYMMDD.md`。结果目录只提交文本
摘要和必要小型表格，原始日志保存在仓库外并记录路径与 hash。
