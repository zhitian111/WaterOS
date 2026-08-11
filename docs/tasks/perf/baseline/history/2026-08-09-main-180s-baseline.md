# 当前 main 180 秒 RISC-V 性能基线（2026-08-09）

## 目的

在 subreaper 修复和 K-05D 复验之后，重新取一次当前 `main`（`77559994`）的早期
BuildStorm 短采样基线，确认最新代码中的热点排序，不直接修改内核。

## 环境

```text
arch=riscv64
kernel=kernel-rv-final
image=sdcard-rv-pub.img
smp=8
memory=8G
snapshot=1
taskset=0,2,4,6,8,10,12,14
timeout=180s
```

命令：

```bash
./scripts/pc-hot/pc-hot-rv.sh run /tmp/pcs-main-rv-180s.txt -- \
  timeout 180 taskset -c 0,2,4,6,8,10,12,14 \
  qemu-system-riscv64 ... -snapshot

./scripts/pc-hot/wait-hot-rv.sh run /tmp/wait-main-rv-180s.txt -- \
  timeout 180 taskset -c 0,2,4,6,8,10,12,14 \
  qemu-system-riscv64 ... -snapshot
```

原始数据：

```text
/tmp/pcs-main-rv-180s.txt
/tmp/wait-main-rv-180s.txt
/tmp/pc-hot-main-rv-180s.log
/tmp/wait-hot-main-rv-180s.log
```

## pc-hot 结果

采样到 `114,518` 个不同 PC。Top 15 符号：

| 排名 | 指令数 | 符号 |
|---|---:|---|
| 1 | 348,737,365 | `with_scheduler`（schedule_tick） |
| 2 | 177,957,812 | `cpu_should_reschedule` |
| 3 | 157,957,883 | `map_page_to_ppn` |
| 4 | 115,044,575 | `??` |
| 5 | 55,200,474 | `u128_div_rem` |
| 6 | 36,110,919 | `with_scheduler`（suspend_current_and_run_next） |
| 7 | 32,706,394 | `TaskRegistry::task_snapshot` |
| 8 | 31,893,537 | `memset` |
| 9 | 21,061,938 | `kernel_global::init` |
| 10 | 20,415,111 | `with_scheduler`（schedule_reschedule） |
| 11 | 16,779,477 | `memcpy` |
| 12 | 16,126,814 | `wateros_kernel_trap_handler` |
| 13 | 15,515,605 | `expire_posix_timers` |
| 14 | 15,457,649 | `expire_realtime` |
| 15 | 15,120,534 | `MultiClassScheduler::schedule` |

## wait-hot 结果

180 秒早期窗口各核 idle：

| CPU | idle_ms | wfi_pc |
|---|---:|---|
| 0 | 175705.272 | `0x80319ebc` |
| 1 | 176483.265 | `0x80319ebc` |
| 2 | 176613.881 | `0x80206bbe` |
| 3 | 177712.204 | `0x80319ebc` |
| 4 | 177792.781 | `0x80319ebc` |
| 5 | 177791.223 | `0x80319ebc` |
| 6 | 177799.120 | `0x80319ebc` |
| 7 | 177818.237 | `0x80319ebc` |

该早期窗口尚未进入完整编译峰值，各核 idle 接近，不能说明完整编译期的负载不均。

## 分析

1. 早期热点集中在调度器：`with_scheduler`、`cpu_should_reschedule`、
   `schedule_tick` 合计远超其它路径。这与此前完整轮中 `with_scheduler` 一直是
   高频符号一致。
2. `map_page_to_ppn` 达到 1.58 亿指令，说明页表映射仍在大规模发生，需进一步
   区分是 COW、ELF 装载、fork 页表复制还是 page fault 路径。
3. `mprotect` 未进入本次 Top 15，和之前 180s 同窗口观察一致：mprotect 只在更长
   完整轮后期才可能成为主要热点。
4. `memset`/`memcpy` 仍是基础库热点，但指令数低于调度器。
5. `expire_posix_timers`/`expire_realtime` 合计约 3 千万指令，值得在完整采样中
   继续观察。

## 下一步候选

- 在 BuildStorm 编译中段再采一轮 pc-hot/wait-hot，确认调度器、页表映射和 mprotect
  随阶段变化的占比。
- 对 `with_scheduler` 和 `cpu_should_reschedule` 做调用次数/锁等待分析，判断是
  10ms tick 调度本身，还是全局锁竞争。
- 对 `map_page_to_ppn` 做调用栈归因，避免只按符号猜测。
- 上述改动涉及 scheduler/task 与 MM 热路径，应保留 task 模块以队友维护为主的原则，
  先提交分析与 A/B 方案，不在缺少完整编译期数据时直接合入调度策略。
