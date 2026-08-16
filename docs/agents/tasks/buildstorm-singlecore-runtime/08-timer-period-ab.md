# BS-SC-08：10/20/50ms timer period A/B

## 任务内容

只调整 `SCHED_TIMER_PERIOD_MS`，验证周期中断是否仍是 BuildStorm 单核瓶颈。不要修改
`MAX_TICKS_PER_TASK`：它主要控制 `SCHED_RR`，普通 rustc 的 `SCHED_OTHER` 不依赖该 quantum。

## 实施方案

1. 从任务 07 的同一提交分别构建 10ms、20ms、50ms 临时候选，保存各 ELF 哈希。
2. 先跑 timer 定向功能：clock/nanosleep/futex/poll/pselect/setitimer/POSIX timer 的提前与
   延后误差；超出既有 10ms 语义的配置直接淘汰，不进入完整性能测试。
3. 通过功能门槛的配置按 RV/LA 交错顺序跑 BuildStorm，避免宿主温度/负载单向漂移。
4. 只把双架构都满足最终功能门槛、且总体优于当前最佳的一个 period 写入 commit。
5. 若 20/50ms 均不合格，恢复 10ms，本任务提交拒绝实验简报，不提交常量变化。
6. 不在本任务实现 tickless/adaptive timer；那需要 scheduler next-deadline 与 enqueue IPI 契约。

## 涉及文件

- `os/components/wateros-base/base-config/src/task.rs`（仅获胜配置）
- timer/sleep/procfs 文档中所有依赖固定 tick 的说明（若常量变化）
- `history/08-brief.md`

## CodeGraph 查询

```bash
codegraph explore "SCHED_TIMER_PERIOD_MS schedule_tick SCHED_OTHER MAX_TICKS_PER_TASK"
codegraph callers "SCHED_TIMER_PERIOD_MS"
codegraph explore "nanosleep futex timeout poll deadline setitimer timer_tick"
```

## 验收命令

每个配置使用独立 target/run-id；最终候选至少执行：

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
python3 scripts/perf/buildstorm_runner.py --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id bs-sc-08-rv-selected-a1 --timeout 1800
python3 scripts/perf/buildstorm_runner.py --arch la --kernel ./kernel-la-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-la-pub-prepared.img \
  --run-id bs-sc-08-la-selected-a1 --timeout 1800
git diff --check
```

`<500s` 可直接保留性能结论，但 timer 功能测试不得省略。

## 完成后简报

新增 `history/08-brief.md`，以表格记录每个 period 的 ELF hash、timer 误差、timer/idle/context
计数、双架构 elapsed、淘汰原因和最终常量。
