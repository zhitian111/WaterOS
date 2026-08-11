# K-10 初赛 musl LTP 全量回归报告（2026-08-01）

## 范围与环境

- RISC-V64、OpenSBI、8 CPU，初赛镜像中的原生 musl LTP（LTP 20240524）。
- 内核：`68276360`（包含调度器实时运行统计修复）。
- 启动入口：`bringup-ltp-musl-only`，超时 7200 秒；每次运行使用全新 qcow2 overlay。
- 本轮不使用自编测试程序；用例语义和结果以镜像内 LTP 二进制、脚本及对应 LTP
  源码为准。

## 结果

- 执行及回收标签均为 2,820 个，完整到达
  `OS COMP TEST GROUP END ltp-musl`。
- 总耗时 2,701 秒；顶层 busybox 命令退出码为 0。
- 2,700 个用例返回 0，120 个返回非零：`rc=1` 90 个、`rc=2` 13 个、
  `rc=3` 1 个、`rc=32` 13 个、`rc=33` 2 个、`rc=127` 1 个。
- 未发现 kernel panic、内核态 page fault 或测试流程卡死。此前 `getrusage04`
  导致的无限循环和随后网络轮询路径故障未复现。
- 脚本结束时清理了 2 个遗留用户任务。这仍是生命周期/测试隔离的残余风险，不能
  因顶层命令成功而忽略。

注意：镜像脚本对每个用例都打印 `FAIL LTP CASE name : rc`，包括 `rc=0`；因此不能按
字符串 `FAIL` 统计失败，必须按末尾返回码判断。

## 非零用例

`rc=32/33` 主要表示 TCONF（当前配置不适用）或 TCONF 与 TFAIL 混合；例如
`epoll_pwait05` 因能力缺失跳过。`memcg_control_test.sh` 的 `rc=127` 是镜像中脚本文件
不存在。它们应与内核语义错误分开处理。

- `rc=1`：`access02`, `acct02`, `chdir04`, `chmod06`, `chown04`,
  `clock_gettime01`, `clock_gettime02`, `clock_gettime04`, `clock_nanosleep01`,
  `clock_nanosleep02`, `clone02`, `connect01`, `epoll_ctl02`, `epoll_ctl03`,
  `epoll_pwait03`, `epoll_wait02`, `epoll_wait03`, `epoll_wait05`, `epoll_wait06`,
  `epoll_wait07`, `execve03`, `futex_cmp_requeue02`, `futex_wait05`, `getcwd01`,
  `getdents02`, `gethostbyname_r01`, `gethostname02`, `getitimer02`,
  `getpeername01`, `getpgid01`, `getrlimit02`, `getrusage02`, `getsockname01`,
  `getsockopt01`, `gettimeofday01`, `kill11`, `lchown02`, `listen01`, `llseek01`,
  `lstat02`, `lstat02_64`, `madvise02`, `madvise10`, `mkdirat02`, `mmap06`,
  `mmap18`, `mmap20`, `pathconf02`, `poll02`, `pread02`, `pread02_64`, `preadv02`,
  `preadv02_64`, `process_vm01`, `pselect01`, `pselect01_64`, `pselect02`,
  `pselect02_64`, `pwrite02`, `pwrite02_64`, `pwrite04`, `pwrite04_64`,
  `pwritev02`, `pwritev02_64`, `readlink03`, `readlinkat02`, `recvmsg01`,
  `rmdir02`, `sbrk01`, `sched_setaffinity01`, `sched_setparam03`, `select02`,
  `setitimer02`, `setpgid01`, `setrlimit03`, `setsockopt01`, `settimeofday02`,
  `shmat02`, `socket01`, `socketpair01`, `stat03`, `stat03_64`, `uname01`,
  `unlink07`, `utimes01`, `waitid04`, `waitid05`, `waitid06`, `waitpid04`,
  `writev01`。
- `rc=2`：`clone08`, `execl01`, `execlp01`, `execv01`, `execvp01`, `getegid01`,
  `getegid01_16`, `prctl05`, `setpgid03`, `sigtimedwait01`, `thp01`, `uname04`,
  `waitpid11`。
- `rc=3`：`select03`。
- `rc=32`：`epoll_pwait05`, `getcontext01`, `gethostid01`, `mallinfo01`,
  `mallinfo02`, `mallinfo2_01`, `mallopt01`, `preadv201`, `preadv201_64`,
  `preadv202`, `preadv202_64`, `pwritev201`, `pwritev201_64`。
- `rc=33`：`asapi_01`, `epoll_pwait04`。
- `rc=127`：`memcg_control_test.sh`。

## 后续判定

本轮证明全量 musl LTP 可以在 SMP 内核上完成，适合作为持续回归基线；它不代表
120 个非零项已经兼容。后续优先处理能覆盖多个用例的共用链路：路径权限与 `*at`
元数据、向量/定位 I/O、等待与进程组、时间和 socket 选项。每项修复先运行对应 LTP
原生用例，再进入下一次全量回归。
