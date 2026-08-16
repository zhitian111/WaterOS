# BuildStorm 单核运行时优化任务

本目录把 syscall/trap/timer/MM 单核优化拆成可独立回归、可独立验收的提交。
任务按编号执行；不得把多个编号压成一个实现提交，也不得在没有功能证据时仅凭墙钟保留优化。

## 工作位置

- 分支：`perf/buildstorm-singlecore-runtime`
- 工作树：`/home/zhitian/project/WaterOS_buildstorm_singlecore`
- 起点：`main@0c4eadf2020c40576baea602ddf5b3d8565b21f5`
- CodeGraph：已初始化于工作树根目录的 `.codegraph/`
- QEMU：`/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/`，必须报告 9.2.1

## 固定输入与基线

| 输入 | SHA-256 |
|---|---|
| `sdcard-rv-pub.img.gz` | `cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1` |
| `sdcard-la-pub.img.gz` | `2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2` |
| `buildstorm_testcode.recovered.sh` | `84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb` |

输入位于 `/home/zhitian/Downloads/`。两份压缩镜像解压后均为 15,032,385,536
字节。原始 `.img.gz` 永远只读；覆写路径是内核实际执行的
`/glibc/buildstorm_testcode.sh`。

用户指定两架构当前最佳记录均按 `520s` 处理，不补跑干净 main 性能 baseline。
已经从起点提交重新构建并保留双架构 Final ELF：

```text
os/tem/perf/buildstorm-singlecore/baseline-main-0c4eadf2/kernel-rv-final
sha256=2f85662abc3ab6987f69066ec282bd7c11af5eaccaa122ad00b583db4ddbb80d

os/tem/perf/buildstorm-singlecore/baseline-main-0c4eadf2/kernel-la-final
sha256=d4a3a7bba55d2c026f28a58f7a4e3fcfdfc42737aace18936349e14126196547
```

这些 ELF 和运行产物位于 Git 忽略目录，不进入提交。

## 两类验收

### 功能与缺陷验收

所有实现提交至少执行最窄的单元测试、受影响架构 check/build 和
`git diff --check`。每个批次结束时必须执行：

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
python3 scripts/tests/test_buildstorm_runner.py
cd ..
git diff --check
```

此外按任务运行 signal、timer、FPU、阻塞/迁移或 MM 定向 guest probe。候选失败而
无法判断是否为既有缺陷时，使用已归档 main ELF 和同一测试镜像做差分；性能 baseline
无需重跑不等于可以省略功能差分。不得新增 panic、SIGSEGV、超时、丢信号、提前唤醒、
FPU/LSX 污染、镜像损坏或结果 marker 缺失。

### 性能验收

- 完整性能结果只认 guest 输出的 `BUILDSTORM_RESULT mode=multi status=OK` 和
  `elapsed_s`；toolchain、minibuild、生成物启动也必须成功。
- QEMU plugin 运行只用于诊断，不参与墙钟验收。
- 每轮从覆写后的母盘建立独立 reflink/稀疏副本，不使用 `-snapshot`，保持用户给定的
  QEMU 9.2.1 命令不变。
- 若宿主磁盘不能同时容纳双架构母盘，按 RV/LA 依次准备、运行和轮换母盘；这只改变宿主
  存储顺序，不改变每轮从固定 `.img.gz` 输入产生独立 raw 副本的要求。
- 单架构 `<500s`：功能门槛通过后直接保留，不为确认性能再重复一轮。
- `500s <= elapsed < 520s`：再跑一轮；两轮都小于 `520s` 才认为该架构改善成立。
- `elapsed >= 520s`：不得作为独立性能收益接受；若只是后续优化的必要前置，可临时保留，
  但必须在简报中标记，最终验收仍要求两架构严格小于 `520s`。
- 最终分支必须在 RISC-V 8 vCPU/16 GiB 和 LoongArch 12 vCPU/36 GiB 上都优于 `520s`。

## 低风险批次策略

任务 01--03 各自形成独立 commit，但只做最窄静态/定向检查；任务 04 对三项累计结果
统一运行双架构功能和性能验收。这样保留可二分、可回退的提交边界，同时避免重复完整测试。
若批次失败，使用逐 commit checkout/bisect 或反汇编计数定位，不以整批回退代替根因分析。

后续任务 05--06 组成 signal 批次，由任务 07 统一验收。高风险架构状态改造按架构单独
验收，不并批。

## 任务顺序

| 任务 | commit 目标 | 风险/验收 |
|---|---|---|
| `00-buildstorm-validation-harness.md` | 建立镜像准备与 QEMU 9.2.1 runner | host 工具 |
| `01-riscv-deduplicate-fpu-restore.md` | 去掉 RISC-V 用户返回重复 FPU restore | 低，批次 A |
| `02-coalesce-return-process-query.md` | 合并用户返回的重复进程状态查询 | 低，批次 A |
| `03-timekeeper-global-signal-timers.md` | 全局 signal timer 仅由 timekeeper 到期 | 低，批次 A |
| `04-low-risk-batch-validation.md` | 批次 A 双架构功能/性能验收 | 验收提交 |
| `05-signal-lifecycle-invariant.md` | 消除返回路径的 signal state ensure | 中，批次 B |
| `06-signal-pending-hint.md` | 无 pending signal 时跳过 registry | 中，批次 B |
| `07-signal-batch-validation.md` | 批次 B 信号并发与性能验收 | 验收提交 |
| `08-timer-period-ab.md` | 10/20/50ms A/B 并只提交获胜配置 | 中 |
| `09-trap-frame-access-guard.md` | 复用权威 trap frame 引用，少一次 scheduler 锁 | 中高 |
| `10-riscv-dirty-fpu-state.md` | RISC-V dirty-only FPU 保存 | 高 |
| `11-loongarch-lazy-lsx-state.md` | LoongArch LSX/FPU 按所有权切换 | 高、条件执行 |
| `12-readonly-mmap-fault-around.md` | 只读文件映射 fault-around | 中高、画像门禁 |
| `13-final-validation-handoff.md` | 双架构最终验收和交接 | 最终门禁 |

## 提交和简报

每个任务恰好对应一个 commit。提交信息使用：

```text
[perf] BS-SC-<NN> <一句话说明>
```

任务完成后必须先新增
`docs/agents/tasks/buildstorm-singlecore-runtime/history/<NN>-brief.md`，再提交实现；
简报和实现属于同一个 commit。简报至少记录：提交 hash（提交后可在下一任务补写完整 hash）、
修改文件、CodeGraph 影响面、实际命令及结果、两架构功能结论、性能结果、保留/拒绝决定、
未验证项和下一任务前置条件。

候选被拒绝时恢复实现代码，但仍以该任务编号提交一份“拒绝实验”简报，避免未来重复试验。
不得提交 `.codegraph/`、`target/`、`kernel-*`、镜像、串口日志和 `os/tem/`。

每个实现提交后执行：

```bash
codegraph sync /home/zhitian/project/WaterOS_buildstorm_singlecore
git status --short
git diff --check HEAD^
```
