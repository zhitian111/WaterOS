# BS-SC-13：最终双架构验收与交接

## 任务内容

冻结候选代码，执行完整功能与性能验收，确认两架构都严格优于 `520s`，整理所有任务简报与
剩余风险。本提交只允许修正文档、测试工具小缺陷和结果记录；发现内核缺陷时返回对应任务
修复，不在最终提交混入未经独立验收的实现。

## 实施方案

1. 冻结候选 HEAD、CodeGraph 索引、双架构 ELF 和准备好的测试母盘哈希。
2. 按“静态检查 → 定向功能 → 完整 BuildStorm → 镜像一致性”的顺序执行，前一步失败即停止。
3. 对 500--520s 区间按规则补跑；小于 500s 不重复性能轮，但仍完成全部功能项。
4. 汇总所有接受、拒绝和条件跳过的任务，更新当前最佳记录与剩余风险后提交最终简报。

## 验收方式

### 功能验收

1. `codegraph sync` 后检查最终影响面和遗漏测试。
2. 双架构 check、Final build、相关局部单元测试全部通过。
3. 重跑最终 signal、timer、FPU/LSX（仅执行过相关任务时）、trap migration 和 mmap probe。
4. BuildStorm 全协议通过：cagent、toolchain、minibuild、timed compile、artifact size、嵌套 QEMU
   启动 `Hello, world!`。
5. 串口结果 marker 前无 kernel panic、SIGSEGV、illegal instruction、OOM、deadlock、stalled、
   文件系统错误或残留任务导致的超时。
6. 对最终运行副本执行 `e2fsck -fn`；母盘和 Downloads 原始压缩镜像哈希保持不变。

### 性能验收

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
python3 scripts/perf/buildstorm_runner.py --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id bs-sc-final-rv-a1 --timeout 1800
python3 scripts/perf/buildstorm_runner.py --arch la --kernel ./kernel-la-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-la-pub-prepared.img \
  --run-id bs-sc-final-la-a1 --timeout 1800
```

- 任一结果 `<500s`：该架构无需性能复跑。
- `500s <= elapsed < 520s`：该架构再跑一轮且两轮均须 `<520s`。
- 任一架构 `>=520s`：最终验收失败，继续优化或回退退化提交，不能交付。

最终报告同时记录 guest elapsed、host wall、timer/idle/context/syscall/page-fault 计数和 ELF/QEMU/
镜像哈希。plugin 画像另跑并标记为诊断，不与墙钟混用。

## 涉及文件

- `history/13-brief.md`
- 本目录 `README.md` 中的最终状态/最佳记录
- 触发文档同步条件时的 `docs/workflows/`、`docs/tools/` 和 `os/scripts/README.md`

## CodeGraph 查询

```bash
codegraph affected $(git diff --name-only 0c4eadf2..HEAD -- 'os/**/*.rs')
codegraph explore "trap syscall signal timer page fault context switch BuildStorm"
codegraph status .
```

## 最终检查

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore
git status --short
git diff --check 0c4eadf2..HEAD
rg -n "待任务|未验证|BLOCKED|失败" \
  docs/agents/tasks/buildstorm-singlecore-runtime/history
```

## 完成后简报

新增 `history/13-brief.md`，逐项列出任务 00--12 的 commit/决定、最终双架构结果与 artifact
哈希、功能矩阵、被拒绝实验、未验证环境和后续建议。只有该简报明确写出两架构 `<520s`
且功能门槛全部通过，本任务才可标记完成。
