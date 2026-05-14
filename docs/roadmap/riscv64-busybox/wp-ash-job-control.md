# 工作包：BusyBox ash — dup2、重定向、后台 `&`、作业与 kill

**所属**：`wateros-syscall`、`wateros-task`、`wateros-ipc`（signal/pgrp 若有）。  
**并行度**：**强依赖** fork/exec/pipe/signal 最小集完成后串行；内部 dup2 与 job 可再拆给人并行。

## 要做什么

1. **`dup2`**：实现 fd 重定向，覆盖 0/1/2；与 shell `>` `<` `>>` 常见路径一致。
2. **进程组 / session（最小）**：若 musl busybox 依赖 `setpgid`/`ioctl(TIOCSPGRP)` 等，按 strace 结果逐项补齐 **stub 或真实现**；每项须在验收中列出。
3. **后台作业 `&`**：`fork` 后父进程不 `wait` 立即继续；需 **作业表或最小追踪** 以支持 `jobs`/`fg` 的降级（首版可只支持 `&` + `wait` 无 hang）。
4. **`kill` 命令**：依赖 `kill` syscall 与 **PID 解析**；与 `wp-ipc-pipe-signal.md` 联调。
5. 针对 **`busybox_cmd.txt`**（赛题或合作仓库）：在文档中维护「已支持命令子集」 checklist，避免与实现脱节。

## 验收要求

- [ ] 在 ext4 根下通过 **BusyBox 二进制**（预编译或自建）执行：`busybox sh -c 'echo hello > /tmp/a && busybox cat /tmp/a'`，串口输出 `hello`。
- [ ] `busybox sh -c 'sleep 1 & wait'` 或等价脚本无死锁、无 panic。
- [ ] 对 **未实现** 的 ioctl/作业命令，行为为 **明确错误信息** 而非静默错误（便于调试）。

## 验证方式

1. bring-up 总线最后阶段：`[bringup][busybox-ash] script=...` 日志；**不**使用 `self_tests`。
2. 将 busybox 与脚本放入 **sdcard 镜像或本地 ext4 镜像** 的固定路径，Makefile 或 `os` 构建文档注明生成方式。
3. 可选：对比同一脚本在 Linux 宿主 + qemu-user 下的 strace 行数，作为「覆盖率」参考（非硬门禁）。

## 依赖

- **上游**：`wp-syscall-process-exec.md`、`wp-ipc-pipe-signal.md`、`wp-syscall-file-io.md`（临时文件写入）。

## 可并行对象

无（本包为 BusyBox 前最后一道集成）；**文档化** busybox_cmd 条目可与实现并行由专人维护。
