# 检查 testsuits QEMU 测例结果

## 任务目标

在 RISC-V QEMU 环境下，按**阶段**串行执行赛题根卷中的 `*_testcode.sh`，收集各组测例通过/失败/PANIC 情况，形成可复用的回归结论，并据此更新路线图与后续 syscall/VFS 修复优先级。

**本任务只负责「跑测 + 判读 + 记录」**，不在同一次对话里默认修内核（除非用户明确要求）。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`（含 **构建与运行**：`cd os && make rv_qemu_run` 等）
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`（§6 Makefile 目标速查）

本任务为运行/判读类，**不需要**预先阅读 `docs/exports/`；若 PANIC 后需规划修复，再按需选读相关组件导出。

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/src/user_bringup_busybox.rs` | **测例开关**：`SCRIPT_PATHS` 分阶段注释 |
| `os/src/user_bringup_bus.rs` | bring-up 总线；确认 `stage-busybox` 已激活 |
| `os/src/user_bringup_common.rs` | 串行 runner、libc 前缀、cwd 设置 |
| `os/scripts/rv_qemu_run.sh` | QEMU 启动参数（块设备、网络、RTC） |
| `os/Makefile` | `make rv_qemu_run` 入口 |
| `os/sdcard-rv.img` | 根卷 ext4 镜像（含 `/glibc`、`/musl` 测例树） |
| `docs/roadmap/test-case-full-pass-plan.md` | 全通过路线图与阶段依赖 |
| `docs/roadmap/testsuits-weekly-plan.md` | 按周计划与里程碑 |

## 辅助脚本（可选）

| 脚本 | 用途 |
|------|------|
| `os/scripts/run_phase_tests.sh` | 自动按 P1→P6 切换 `SCRIPT_PATHS` 并逐阶段 `make rv_qemu_run` |
| `os/scripts/parse_qemu_test_log.py` | 从 QEMU 日志提取各 `*_testcode.sh` 的摘要 |

## 搜索范围

- `os/src/user_bringup_*.rs`
- `os/scripts/rv_qemu*.sh`
- `os/components/wateros-syscall/**`（PANIC 时查 dispatch 表与 nr）
- 根卷内脚本（用 `debugfs -R "cat /glibc/…" os/sdcard-rv.img` 查看，无需挂载）

## 输出目录

- **运行日志**：建议保存到 `/tmp/wateros_phase_runs/<P*.log>` 或 `os/log`（勿提交 git）
- **结论回填**：
  - `docs/roadmap/test-case-full-pass-plan.md`（勾选清单、已知失败项）
  - `docs/roadmap/testsuits-weekly-plan.md`（里程碑进度，若当次有阶段验收）
  - `docs/roadmap/todolist.md`（若暴露新的 syscall/子系统缺口）

## 并行拆分策略

**不要**在一次 QEMU 启动里启用多个阶段或全部 24 个脚本——未实现的 syscall 会 **直接 panic**，后续脚本不会执行。

推荐拆分方式：

| 并行维度 | 说明 |
|----------|------|
| **按阶段 P1–P6** | 每次只取消注释一个 `// --- P* ---` 块 |
| **按 libc** | 同一阶段内可先只跑 `/glibc/…`，再跑 `/musl/…`（便于对比） |
| **P5 子拆分** | `libctest` 与 `cyclictest` 分开启用（cyclictest panic 不应掩盖 libctest 结果） |
| **P6 单独** | LTP 用例量大，单独长跑，勿与其他阶段合并 |

同一阶段内 glibc/musl 两条脚本可在一次 `make rv_qemu_run` 中串行执行（当前 runner 设计即如此）。

## 阶段与 `SCRIPT_PATHS` 对应关系

在 `os/src/user_bringup_busybox.rs` 中维护：

| 阶段 | 脚本 | 典型依赖 |
|------|------|----------|
| **P1 basic** | `basic_testcode.sh` × glibc/musl | 文件 IO、进程、内存 syscall；`mount` 需第二块盘 |
| **P2 busybox + lua** | `busybox_testcode.sh`、`lua_testcode.sh` | shell 管道、ioctl TTY、`readv` 等 |
| **P3 benchmark** | lmbench / unixbench / libcbench / iozone | `getrusage`、信号、多进程、向量 IO |
| **P4 网络** | iperf / netperf | **QEMU 需 virtio-net**；`setsid` 等 |
| **P5 libctest + cyclictest** | libctest / cyclictest × glibc/musl | 动态链接、TLS、定时器、`get_mempolicy` 等 |
| **P6 LTP** | `ltp_testcode.sh` | 范围最广，依赖 busybox 命令与大量 syscall |

详细顺序见 `docs/roadmap/test-case-full-pass-plan.md` 第四节。

## 标准执行流程

### 1. 确认环境与开关

```bash
cd os
# 确认 sdcard 镜像存在
test -f sdcard-rv.img
# 确认 bring-up 总线已挂载根卷并激活 stage-busybox（user_bringup_bus.rs）
```

在 `user_bringup_busybox.rs` 中：**仅取消注释目标阶段的脚本行**，其余保持注释。

### 2. 按需调整 QEMU（P4 / mount）

- **P4 网络**：在 `os/scripts/rv_qemu_run.sh` 中取消注释 `virtio-net` 与 `-netdev user`
- **P1 mount**：赛题要求第二块 `virtio-blk`（`disk.img`）；单盘时 `mount` 测例预期 `-22`

### 3. 运行

```bash
cd os
make rv_qemu_run 2>&1 | tee /tmp/wateros_P1.log
```

测例跑完后内核会 **主动关机**，QEMU 正常退出。

或使用阶段批跑（会临时改写 `user_bringup_busybox.rs`，结束后恢复备份）：

```bash
cd os
bash scripts/run_phase_tests.sh
```

### 4. 解析日志

```bash
python3 os/scripts/parse_qemu_test_log.py /tmp/wateros_P1.log
```

若出现 PANIC，额外提取 syscall 号：

```bash
grep -E 'unsupported: unknown nr=|Panicked at' /tmp/wateros_P1.log
```

riscv64 常用对照（遇 panic 时查表）：65=`readv`，123=`sched_getaffinity`，157=`setsid`，165=`getrusage`（236=`get_mempolicy` 已 stub）。

## 各组测例的「通过」判读标准

**不能**仅凭 `#### OS COMP TEST GROUP START/END ####` 判断通过；必须看组内具体输出。

| 组别 | 通过标志 | 失败 / 未执行标志 |
|------|----------|-------------------|
| **basic** | 每个 `Testing <name> :` 后有 `========== END test_<name> ==========`，且无 `--- Assert Fatal ! ---` | `Assert Fatal`；`mount return: -22` 等 |
| **busybox** | 每一行 cmd 对应 `testcase busybox <line> success`（`busybox_cmd.txt` 约 55 行） | `testcase … fail`；**START/END 之间无任何 testcase 行** = 管道/循环未跑 |
| **lua** | `testcase lua <脚本> success` | `fail` 或 LoadPageFault |
| **libctest** | 大量 `Pass!`；START/END 成对 | 页故障、脚本 exit 非 0 |
| **LTP** | `FAIL LTP CASE <name> : 0` | 非 0 返回；`basename: not found` 等环境缺命令 |
| **benchmark / 网络** | 脚本内子项有正常数值输出且无 PANIC | 中途 `unsupported: unknown nr=` |

### busybox 特别说明

脚本逻辑（根卷 `/glibc/busybox_testcode.sh`）：

```sh
./busybox cat ./busybox_cmd.txt | while read line; do
  eval "./busybox $line"
  # 成功则打印 testcase busybox $line success
done
```

仅出现 START/END、**没有** ~55 条 `testcase busybox …` 时，表示 **while 循环未消费任何命令**，不能记为通过。

## PANIC 后的处理约定

1. 记录 **阶段名、脚本路径、syscall nr、调用栈/模块**
2. **不要**在同一次运行中继续依赖后续脚本结果（内核已 panic）
3. 下一回归只启用**下一个未测阶段**或**修复后重跑当前阶段**
4. 将 nr 映射到 `wateros-syscall` dispatch / `todolist.md` 待办

## 完成后的回填要求

- 更新 `docs/roadmap/test-case-full-pass-plan.md` 第五节勾选清单（增量）
- 若某阶段达到里程碑，更新 `docs/roadmap/testsuits-weekly-plan.md` 对应周验收项
- 新暴露的 syscall 缺口写入 `docs/roadmap/todolist.md`
- **不要**把 `/tmp/*.log`、`os/log` 提交进 git
- 测完恢复 `user_bringup_busybox.rs`：仅保留当前回归基线阶段的注释状态（避免误提交「全开」配置）

## 推荐回归顺序（与路线图一致）

```
P1 basic（修 mount）
  → P2 busybox + lua
  → P3 benchmark
  → P4 网络（先改 QEMU 再测）
  → P5 libctest（单独）→ cyclictest（单独）
  → P6 LTP
  → LoongArch / 四套交叉（另任务，见 test-case-full-pass-plan P6）
```

## 已知环境限制（记录结论时需注明）

| 限制 | 影响 |
|------|------|
| 单 virtio-blk | basic `mount` 失败（`-22`） |
| 未启 virtio-net | P4 iperf/netperf 无法真实测通 |
| 未实现 syscall | 直接 panic，同次运行后续脚本无效 |
| LTP 体量 | 需单独长跑；中断则只能记「部分」 |

## 任务完成自检清单

- [ ] 仅启用了一个阶段（或明确记录了分阶段多次运行的合并结论）
- [ ] 保存了 QEMU 完整日志路径
- [ ] 每组测例使用了**正确的判读标准**（非仅 START/END）
- [ ] PANIC 已记录 syscall nr 与触发脚本
- [ ] 路线图 / todolist 已增量更新
- [ ] `user_bringup_busybox.rs` 注释状态符合团队约定（非 accidental 全开）
