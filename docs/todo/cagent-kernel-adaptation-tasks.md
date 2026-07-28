# CAgent 内核适配任务说明

## 目标与范围

目标是在 RISC-V64、QEMU `virt`、8 核和决赛镜像
`os/sdcard-rv-pub.img` 上稳定通过 CAgent 十项并发测试，同时保证
`test_case/sdcard-rv.img` 的初赛测试不发生内核 panic、trap 循环或永久卡死。

需求依据为：

- `final_test_case/scripts/cagent_testcode.sh`
- `final_test_case/cagent-test/agent_lite.c`
- `final_test_case/cagent-test/simple_llm_server.c`
- `final_test_case/judge/judge_cagent-glibc.py`

不通过修改 judge、伪造输出或改变 Linux syscall 语义取得通过。

## 测试行为与内核能力

| 测试 | 服务端生成的命令 | 主要内核能力 |
| --- | --- | --- |
| factorial | `echo 3628800` | fork/exec、pipe、wait、stdio |
| date | `date -d ...` | 实时时钟、动态 ELF、pipe |
| network | `ss -tan ...` | TCP、proc/net 或 netlink 兼容 |
| cpu | `nproc` | online CPU、`sched_getaffinity` |
| kernel | `uname -r` | `uname`、fork/exec/wait |
| fs-create | `printf ... > file` | create/truncate/write/close |
| fs-readwrite | `printf` + `awk` | 并发文件 I/O、pipe、exec |
| fs-directory | `mkdir` + `touch` + `ls` | mkdir、`utimensat`、getdents |
| fs-search | `find` + `wc` | 递归 metadata/getdents、pipe |
| fs-usage | `df` + `awk` | `statfs`、pipe、exec |

每个 agent 会建立两条到 `127.0.0.1:8080` 的 TCP 连接：第一条取得工具
调用，执行 shell 命令后，第二条提交工具结果。十个 agent 同时运行，因此一次
完整测试至少包含 20 次 TCP connect/accept/send/recv/close。

服务端调用 `listen(fd, 10)` 并单线程顺序处理连接。它依赖
`SO_REUSEADDR`、阻塞 accept/read、信号中断和正确的 TCP 关闭状态机。
agent 的 `popen()` 依赖 fork/exec、pipe、fd 继承、SIGCHLD 和 wait 语义。

## 当前状态

### 已满足

- 动态解释器路径和脚本 cwd 已由 `327f02af`、`fc0d84cf` 修复。
- 用户任务初始化后发布，提交 `72cbf633`。
- `sigsuspend` 在信号投递前保留临时 mask，提交 `526b2e0e`。
- ext4/page-cache 并发加固由 `fba81834` 完成。
- `touch` 所需 `utimensat` 由 `6e7e04dd` 实现。
- 当前运行中 factorial、date、network、cpu、fs-create、fs-search 和
  fs-usage 已通过；kernel、fs-readwrite、fs-directory 在超时边界失败。
- kernel 与 fs-directory 单项探针均能成功，说明剩余失败不是命令或 syscall
  完全缺失。

### 已确认阻断

#### CAG-NET-01：监听 backlog 小于测试并发数

`simple_llm_server.c` 请求 backlog 10，但
`wateros-driver/driver-network/src/lib.rs` 将监听槽数限制为 6。首轮并发会耗尽
监听槽，当前运行只记录 17/20 次 HTTP 请求，三个 agent 在 20/30 秒超时。

需要：

- 使有效监听槽至少覆盖请求的 backlog 10，并保留合理上限。
- 分配多个监听槽失败时回滚已经创建的 socket/meta，不能泄漏半初始化组。
- 验证连续多轮 CAgent 都出现 20 次请求、10 项 pass 和 GROUP END。

#### CAG-NET-02：本地 TCP burst 前进与等待

当前阻塞 connect/accept/send/recv 通过“poll 一次、sleep 一个 tick”推进协议栈，
没有基于 socket 状态的事件唤醒。单项可运行但十项并发耗时约 4 秒，并在监听槽
不足时出现长尾。

需要在 CAG-NET-01 后复测；若仍丢请求，再为本地 TCP 握手/关闭增加有限 burst
poll 或状态事件等待。不得忙等，也不得让一个 socket 唤醒无关任务。

## 测试侧差异

仓库脚本的无参数 `wait` 会同时等待常驻 server，按 Linux 语义会永久等待。
当前决赛镜像中的脚本已经使用 `TEST_PIDS`，只等待十个测试任务后再 kill server。
内核必须按正常 wait 语义实现，不能迁就旧脚本。

client/server 对 partial `send/read/write` 的处理不完整，但同一源码在 Linux 上
连续 20 轮、200 个 agent、400 次请求全部完成。它是测试程序的健壮性风险，
不是当前 WaterOS 丢请求的充分解释。

## 回归要求

每个修复条目独立提交，并至少执行：

1. `make kernel-rv-final`
2. 决赛镜像 CAgent 十项并发测试，记录 pass/reject、请求数和 GROUP END。
3. `make kernel-rv-pre`
4. 使用 `test_case/sdcard-rv.img` 的只读 backing/overlay 启动初赛测试，确认
   无 panic、异常 trap 循环或无进展卡死。

文件系统问题由对应成员处理；若运行证据指向 VFS/ext4，应记录最小复现和日志，
不与网络或 task 修复混在同一提交。
