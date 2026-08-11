# CAgent 内核适配工作汇总

## 目标与依据

本轮以 `final_test_case/scripts/cagent_testcode.sh`、
`cagent-test/agent_lite.c`、`simple_llm_server.c` 和 judge 源码为准，目标是在
RISC-V64 QEMU `virt`、OpenSBI、8 核环境稳定通过十项 CAgent 并发测试，同时不让
`test_case/sdcard-rv.img` 的初赛路径发生 panic、异常 trap 或无进展卡死。

任务说明和分类计划分别见：

- `docs/todo/cagent-kernel-adaptation-tasks.md`
- `docs/todo/cagent-kernel-adaptation-plan.md`

## 已完成更改

### 网络

- `dba747a2`：兑现 `listen(backlog)`，将监听槽上限提高到 16，并使多槽初始化失败
  可回滚。
- `1fec182a`：回环 TCP 禁用 Nagle；大于 MSS 的发送在 syscall 路径执行有界协议栈
  推进，避免尾段长期等待。
- `0d25a32b`：为 accept/补充 listener 的状态转换保留额外槽位。

涉及 `os/components/wateros-driver/driver-network/src/lib.rs`、
`sys/fs/io.rs` 和 `sys/net/sendto.rs`。

### Task 与调度

- `bddd4979`：先发布 task/process 退出状态，再通知父任务和 waiters，消除“已唤醒
  但仍看不到退出状态”的竞态。
- `a79c8d8c`：timekeeper tick 唤醒本 CPU sleeper 后立即兑现重调度。
- `d43500a6`：timer tick 始终检查本 CPU `need_resched`，使硬件 IPI 仅作为加速；
  IPI 被合并或延迟时，下一 tick 仍能调度已唤醒任务。

### 启动与诊断

- `5146c20a`：RISC-V/LoongArch QEMU UART flush 等待 `TEMT`，panic reset 前先 flush，
  避免最后一行诊断被固件复位截断。
- `76672612`：pre 启动不再同步执行 4,662 次 LTP unlink；保留现有 exec fast-exit
  防止排除项 worker 永久阻塞。

## 验证结果

`make kernel-rv-final`、`make kernel-rv-final-smp-test` 和
`make kernel-rv-pre` 均构建成功。

最终 CAgent 使用 `os/sdcard-rv-pub.img` 的全新 qcow2 overlay 连续运行三轮：

| 轮次 | HTTP 请求 | 结果 | GROUP END | 客体耗时 |
| --- | ---: | ---: | --- | ---: |
| 1 | 20/20 | 10/10 pass | 有 | 12.204 s |
| 2 | 20/20 | 10/10 pass | 有 | 13.058 s |
| 3 | 20/20 | 10/10 pass | 有 | 13.228 s |

每轮之前的 32 后台任务 SMP 文件压力均打印 `SMP_MM_TEST_DONE`，日志未出现 panic、
OOM 或异常 trap。

pre 使用 `test_case/sdcard-rv.img` 的 4 KiB-cluster overlay。180 秒窗口内 glibc
和 musl cyclictest 均完成，LTP 连续启动 248 项、完成 247 项；最新调度器的 60 秒
复测连续启动 167 项、完成 166 项。两次均因宿主 `timeout` 主动结束，结束前仍在
运行下一项，无 panic、OOM 或异常 trap。

## 已知限制

- 未执行约 35–45 分钟的完整 pre 队列；本轮结论是“未发现卡死或崩溃”，不是
  “全部初赛功能通过”。
- pre 日志仍有 `/dev/null` 路径提示及若干 LTP 功能性 TFAIL，例如协议表、clone
  参数和 TCP errno 差异，应按 syscall/devfs 条目继续处理。
- 运行时只验证了 RISC-V；LoongArch UART 改动保持同一 16550 语义，但未做镜像启动
  验证。
- 仓库既有编译 warning 未在本轮顺带清理。
