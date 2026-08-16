# BS-SC-07：signal 批次 B 统一验收

## 任务内容

统一验收任务 05--06 的生命周期 invariant 与 pending hint。本任务完成前，任何偶发丢信号、
退出卡死或 registry 泄漏都视为阻断问题，不能用性能收益抵消。

## 实施方案

1. 固定任务 06 后的候选 HEAD，并生成双架构 Final ELF。
2. 先运行确定性 signal 状态机测试，再运行跨 CPU 并发压力和生命周期泄漏检查。
3. 功能全部通过后运行双架构 BuildStorm 与 syscall/锁计数诊断。
4. 根据功能与性能证据接受整批、修复具体提交或逐项回退，并在简报中关闭所有待验收项。

## 验收方式

### 功能验收

1. 双架构 check/build。
2. 每架构至少运行：handler+rt_sigreturn 1000 次、blocked/unblocked 10,000 次、线程定向与
   进程定向各 10,000 次、sigsuspend/ppoll mask race、signalfd、SIGCHLD、SIGSTOP/CONT/KILL。
3. 将 sender 固定到另一 CPU，目标线程反复 syscall 返回/阻塞/迁移；结束后发送数、handler
   数、signalfd 消费数和预期一致。
4. 循环 fork/clone/exec/exit 后 signal/process/task registry 有界。
5. 若候选与预期不一致，用归档 main ELF 跑同一 probe；只允许修复既有失败，不允许新增失败。

### 性能验收

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
python3 scripts/perf/buildstorm_runner.py --arch rv --kernel ./kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id bs-sc-07-rv-a1 --timeout 1800
python3 scripts/perf/buildstorm_runner.py --arch la --kernel ./kernel-la-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-la-pub-prepared.img \
  --run-id bs-sc-07-la-a1 --timeout 1800
```

按总 README 的性能规则决定是否补跑。配套 syscall-profile/诊断计数应证明无信号 syscall
不再获取 signal registry，process registry 次数也符合任务 02/05 的设计。

## 涉及文件

- `history/07-brief.md`
- 必要的 `os/scripts/perf/guest-probes/signal_*.sh`
- 仅在确认工具缺陷时修改 runner/probe 测试

## CodeGraph 查询

```bash
codegraph affected os/components/wateros-task os/components/wateros-ipc/ipc-signal \
  os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs
codegraph explore "signal mask pending deliverable wake interrupt reschedule"
```

## 完成后简报

新增 `history/07-brief.md`，列出任务 05--06 commit、全部信号计数、registry 最终大小、性能
result.json 路径/哈希、当前两架构最佳记录和保留决定。
