# 线下测试、LTP 与 benchmark 执行手册

本文说明如何选择测试、保存证据并避免把“脚本跑完”误报为“功能通过”。已有一次完整数据整理见 [RISC-V LTP/benchmark PPT 数据底稿](../../os/docs/reports/2026-08-18-ltp-bench-ppt-report.md)。

## 1. 四类结果必须分开

| 层 | 成功标志 | 失败标志 |
| --- | --- | --- |
| 构建 | 命令 exit 0 | 编译/链接失败 |
| QEMU/内核 | 正常启动和预期 shutdown | QEMU 参数错误、panic、hang |
| guest wrapper | 顶层脚本 exit 0、结束 marker | script timeout/nonzero/missing |
| 测试断言 | LTP TPASS/TFAIL/TBROK/TCONF 或 benchmark 有效输出 | 断言失败、无样本、数据无效 |

顶层 `exit_code=0` 只证明 wrapper 结束。LTP 内有 `TFAIL`、iozone 的 fsync 失败、cyclictest 无样本时，都不能写“全部通过”。

## 2. 每次运行的记录头

```text
date/time:
git revision + git status/diff:
ARCH / PROFILE / MODE / SMP:
kernel features:
QEMU version / host OS:
WOS_QEMU_MEM:
SDCARD path + checksum:
SNAPSHOT / WRITE_DISK:
guest libc and test path:
exact command:
log path:
expected completion marker:
timeout and stop reason:
```

性能对比只允许改变一个变量。特别是 1 GiB 与 8 GiB、SMP=1 与 SMP=8、snapshot 与真实写盘都不是同口径。

## 3. 回归阶梯

### 第 0 层：格式与 host 工具

```sh
git diff --check
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 scripts/maintenance/check_offline_docs.py
```

host 测试可能依赖操作系统路径规范。macOS 的临时目录真实路径常带 `/private/var`，断言若直接拿 `/var` 比较会出现平台差异；先判断是测试可移植性还是产品逻辑错误。任何失败都要记录测试名和 traceback，不能只报告总数。

组件 crate 的 Rust host 单测优先从它所属的最小 workspace/manifest 启动。例如 block cache：

```sh
cargo test \
  --manifest-path components/wateros-driver/driver-block/block-impl/impl-block-cache/Cargo.toml \
  --no-default-features
```

不要默认认为顶层的 `cargo test -p <package>` 等价。顶层 workspace 使用的 feature resolver 和
默认 WaterOS feature 可能把 platform/RISC-V 实现合入依赖图；在 x86_64/macOS host 上会先因
`sbi-rt` 的 `a0..a7` 寄存器报错，测试函数根本尚未编译。判断方法：看第一条错误属于被测 crate
还是架构依赖，并用 `cargo tree` 检查 feature 来源。切到子 manifest、关闭非必要默认 feature
后才能把“host 不支持目标汇编”与真实单元测试失败分开记录。

独立 manifest 可能在子 workspace 生成被 `.gitignore` 忽略的 `Cargo.lock`/`target`；提交前仍用
`git status --ignored` 确认没有误纳入产物。host 单测通过也不替代 RV/LA target check。

### 第 1 层：完整目标 check

```sh
make check ARCH=rv PROFILE=pre
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=pre
make check ARCH=la PROFILE=final
```

check 证明 feature 组合可编译，不证明启动和运行语义。warning 应保留数量/类别；本次改动新增 warning 才是回归重点。

### 第 2 层：单核定向 shell

```sh
make shell ARCH=rv PROFILE=final SMP=1
```

单核用于减少竞争变量，运行最小 raw syscall、小文件、单进程/线程用例。确认失败可重复后再上 SMP。

### 第 3 层：SMP 定向脚本

将 guest 验证写入镜像中的绝对路径，然后：

```sh
make run ARCH=rv PROFILE=final SMP=8 \
  MODE=run SCRIPT=/root/regression.sh
```

脚本应打印唯一 BEGIN/CASE/RESULT/END marker，每个 case 独立记录 rc。不要只依靠最后一个 shell `$?`。

### 第 4 层：完整 auto 队列

```sh
make run ARCH=rv PROFILE=pre MODE=auto SMP=8
make run ARCH=rv PROFILE=final MODE=auto SMP=8
```

auto 队列由根镜像标志选择，不只由 PROFILE 决定。核对日志中的 detected image 和实际命令列表。

### 第 5 层：重复/长时压力

至少跑两轮，比较第二轮前后对象数和内存。第一次运行包含动态链接、页缓存和一次性 registry 分配，不能单独据此判泄漏。

## 4. operator 冒烟

[`operator_smoke.py`](../../os/scripts/testing/operator_smoke.py) 驱动串口 PTY，覆盖：shell prompt、pipe/file、后台 wait、Ctrl-C、raw TTY、shell 退出后救援 shell；可选 Vim。

```sh
python3 scripts/testing/operator_smoke.py \
  --arch rv --profile final --smp 1 --log /tmp/operator-rv-final.log
```

它会启动/终止自己的 QEMU 进程组并保存串口日志。失败时看最后成功 marker，而不是只看 Python exception。`--no-build` 只能在确认 kernel 与当前源码/config 一致时使用。

## 5. LTP 定向回归

[`guest_read_family_regression.sh`](../../os/scripts/testing/guest_read_family_regression.sh) 是 read/pipe/socket/eventfd 的 guest 侧稳定 marker 样板：

```text
READ_FAMILY_BEGIN
READ_FAMILY_CASE_BEGIN case=<name>
READ_FAMILY case=<name> ok=true|false rc=<n>
READ_FAMILY_RESULT passed=<n> failed=<n> missing=<n>
```

可以通过环境变量换目录、timeout 和 case 集合。新增模块定向脚本也应使用这种逐 case marker，而不是打印固定的 `FAIL ...` 字样再依赖不可靠的 shell 状态。

LTP 判定优先级：

1. `TBROK`：测试基础设施或前置条件断裂；
2. `TFAIL`：断言失败；
3. `TCONF`：环境不支持，不能计成功也通常不算内核失败；
4. `TPASS`：对应子断言通过；
5. case/wrapper rc 与 Summary：辅助校验完整性。

[`ltp_sum_passed.py`](../../os/scripts/testing/ltp_sum_passed.py) 能汇总 Summary 的 passed 数和 RUN case 数：

```sh
python3 scripts/testing/ltp_sum_passed.py /tmp/wateros-ltp.log
```

它不替代 TFAIL/TBROK 分类。`TPASS` 行数也只是参考，同一个 case 可包含多个断言。

## 6. LTP hang 定位与镜像裁剪

[`ltp_hang_iterate.sh`](../../os/scripts/testing/ltp_hang_iterate.sh) 会修改镜像、skip 表并反复构建，属于有副作用的历史工具。使用前必须复制镜像并确认 git 工作区；它不适合与其他 QEMU 测试并行。

[`ltp_prune_sdcard_before.sh`](../../os/scripts/testing/ltp_prune_sdcard_before.sh) 通过 debugfs 删除目标 case 之前的二进制，以便从某位置重跑。先用 `--dry-run`：

```sh
./scripts/testing/ltp_prune_sdcard_before.sh \
  --img /tmp/sdcard-rv-ltp.img --before mmapstress01 --libc musl --dry-run
```

原则：

- 永远操作明确的镜像副本，不对基线镜像直接写。
- 保留原始完整日志、裁剪列表和起始 case。
- hang 的判据必须包含“最后 marker 多久未变化”和宿主/guest CPU 状态。
- 被人工终止的 case 标为 timeout/hang，不标 TFAIL，除非测试自身产生 TFAIL。

## 7. 历史 phase/perf 脚本警告

`run_phase_tests.sh`、`run_iozone_minimal.sh`、`run_perf_bringup_phases*.sh` 与 `min_accept_execve_lazy.sh` 会临时改写 `src/user_bringup_busybox.rs`，且部分脚本仍搜索旧版 `BRINGUP_COMMANDS` marker 或调用兼容 Make target。当前源码已改为 `PRELIMINARY_COMMANDS/FINAL_COMMANDS` 和 operator 模式，运行前必须先读脚本并确认 marker 存在。

这些脚本依靠 trap 恢复文件，但宿主崩溃或强制 kill 仍可能留下 `.bak` 或半修改源码。运行前后都执行：

```sh
git status --short src/user_bringup_busybox.rs
git diff -- src/user_bringup_busybox.rs
```

新测试优先使用 `MODE=run SCRIPT=/absolute/guest/path`，避免自动改写 Rust 源码。

## 8. QEMU 日志解析

[`parse_qemu_test_log.py`](../../os/scripts/testing/parse_qemu_test_log.py) 面向旧日志格式，按 `[busybox-bringup] script_path` 切块，并把 `FAIL LTP CASE name : 0` 当通过。当前 bring-up 日志和 LTP wrapper 可能不同，使用前先检查 parser 是否实际识别到 block；输出 total=0 不是“零失败”，而是很可能没匹配格式。

可靠解析器应：

1. 去 ANSI 色码但保留原日志；
2. 以明确 BEGIN/END marker 切块；
3. 检测缺失 END、panic、kill 和 QEMU 非零退出；
4. LTP 单独统计 TFAIL/TBROK/TCONF/TPASS 和 case 数；
5. benchmark 验证样本数和单位；
6. 输出 machine-readable JSON/CSV 以及人类摘要；
7. 把无法判定标 UNKNOWN，而不是 PASS。

## 9. MM/task/IPC 压力

### forkheavy

```sh
cat /proc/meminfo
stress-ng --forkheavy 4 --timeout 60s --metrics-brief
cat /proc/meminfo
```

记录 stressor rc、fork/exit/reap 计数、heap used/free、task/zombie、aspace/VMA、fd/OFD/pipe、futex waiter、signal/robust/SHM 表项。只看 `/proc/meminfo` 的 guest 物理内存不足以判断固定内核 heap。

### mmap/file writeback

```sh
stress-ng --mmap 1 --mmap-file --mmap-bytes 16M --timeout 10s
```

另写一个确定性用例：创建文件、shared mmap、跨页修改、msync/munmap、关闭、重开校验；再让进程不显式 munmap 直接 exit。两条路径都不能出现重复销毁或 writeback warning。

### pipe/eventfd/socket

必须包含坏用户指针回滚、signal interrupt、nonblock、poll/epoll、dup/fork 和最后关闭。吞数据、永久 reservation、持锁睡眠通常只在这些组合中出现。

## 10. 文件系统与持久化

snapshot 模式验证的是 guest 本轮可见性，不证明落盘。持久化回归使用可恢复副本：

```sh
cp /path/base.img /tmp/wateros-fs-regression.img
make shell ARCH=rv PROFILE=final \
  SDCARD=/tmp/wateros-fs-regression.img SNAPSHOT=0 WRITE_DISK=1
```

guest 内执行 create/write/fsync/rename/unlink，然后正常 shutdown，重新启动读回。保留 fsync 与 block flush 日志；普通 write throughput 成功但 fsync `EIO` 时只能报告“缓存写入完成、持久化失败”。

## 11. benchmark 口径

### cyclictest

记录 timer 精度支持、线程 affinity、priority、每线程样本数。样本数为 0 的线程没有 Avg/Max，不能纳入平均。压力前后比较使用相同 SMP 和 workload。

### libcbench

保留每个 micro-benchmark 原始名称、耗时和内存单位。总运行时间不能替代单项结果；malloc、pthread、string/stdio 应分组展示。

### lmbench

确认被 exec 的 helper 路径存在。日志出现 `not found` 时，对应 fork+shell 等数据无效，即使 benchmark 打印了数值。区分 latency 的 μs 与 bandwidth 的 MB/s/KB/s。

### iozone

记录 file size、record size、进程数、parent/children 口径、缓存/同步选项。自动模式失败后单项模式完成，不能把两者合并成“iozone 完整通过”。

## 12. PPT 数据表模板

| 字段 | 内容 |
| --- | --- |
| Test group/case | 原始稳定名称 |
| Environment ID | arch-profile-SMP-memory-image checksum |
| Command | 完整 guest 命令 |
| Start/end/elapsed | 单调时间优先 |
| Samples | 有效样本数 |
| Metric/unit | 原始值与单位 |
| Wrapper rc | 顶层返回值 |
| Assertions | pass/fail/brok/conf |
| Kernel anomalies | panic/OOM/warning 数 |
| Validity | valid/partial/invalid/unknown |
| Notes | 缓存预热、路径缺失、fsync 失败等 |

图表只使用 `valid` 数据；`partial` 可展示但必须标注限制。一次运行的原始日志不可覆盖，整理后的表要能反向定位原行。

## 13. 提交记录模板

```text
Change:
Risk owner/lifecycle:

Static:
- rv/pre check: PASS|FAIL
- rv/final check: PASS|FAIL
- la/pre check: PASS|FAIL
- la/final check: PASS|FAIL
- host tests: passed/failed/total + failed names

Runtime:
- minimal case:
- SMP case:
- repeated stress:
- full queue:

Resources before/after:
Known warnings:
Not run and reason:
Log paths/checksums:
Conclusion: PASS|PARTIAL|FAIL
```

只要还有未运行项或已知异常，结论应为 PARTIAL。这样下一位线下开发者可以从明确缺口继续，而不是重新猜测“应该已经测过”。
