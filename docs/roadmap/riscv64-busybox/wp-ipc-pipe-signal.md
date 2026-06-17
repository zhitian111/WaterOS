# 工作包：wateros-ipc — pipe 与最小 signal（BusyBox 前置）

**所属**：`os/components/wateros-ipc`（pipe、signal 子 crate 接入聚合层）、`wateros-syscall`。  
**并行度**：pipe 可在 **fd 表** 定型后独立实现；signal 与 **进程包** 紧密相关，建议后半段串行。

## 要做什么

1. **`pipe2`/`pipe`**：创建一对 fd，接入 per-task fd 表；支持 **阻塞读/写** 与 **非阻塞** 可先二选一（文档说明）。当前状态：内核 ring-buffer pipe 已完成，最小 fd/syscall 接入与单任务 pipe smoke 已完成；fork/dup/关闭继承语义待进程包联调。
2. **最小 signal**：
   - `rt_sigaction`、`rt_sigprocmask`、`rt_sigreturn` 中与 **忽略/默认** 及 **单 handler** 相关的子集；
   - `kill`/`tkill` 之一，能向 **子进程** 投递至少 `SIGCHLD` 相关路径或测例所需信号（与 `wp-ash-job-control.md` 对齐）。
3. 将 `wateros-ipc` 从 **默认 dummy** 转为可选 **active impl**（feature 开关），默认构建路径须 **开启** BusyBox 所需最小集。
4. 与 **trap 返回用户** 路径协作：信号可打断 syscall 时 errno（`EINTR`）策略需文档化。

## 验收要求

- [x] 用户程序：单任务 `pipe` → `write` → `read` → `close` 得到固定字节串。
- [ ] 用户程序：`pipe` → fork → 子 `write` 父 `read` 得到固定字节串。
- [x] 内核路径：handler frame、trampoline、`rt_sigreturn`、mask 恢复及 `SA_RESTART` 已接入双架构公共返回路径。
- [ ] 用户程序：安装 handler 后 `kill(getpid(), SIGUSR1)`（或约定信号）能观察到 **用户态 handler 运行**（仍需在可正常装载动态 ELF 的 RISC-V 测试环境验收）。
- [ ] 未实现信号集全量功能时，对未支持 syscall 仍返回 **`ENOSYS`**，不 panic。

## 验证方式

1. bring-up 总线：`[bringup][ipc-pipe] PASS`、`[bringup][ipc-signal] PASS` 分两子阶段。
2. 与 **进程包** 联调：`sh -c` 形态在 `wp-ash-job-control.md` 验收。
3. 压力：快速连续 pipe 创建/关闭 100 次无泄漏（fd 数或内核日志计数）。

## 依赖

- **上游**：`wp-syscall-file-io.md`、`wp-syscall-process-exec.md`（fork + fd）。
- **下游**：`wp-ash-job-control.md`。

## 可并行对象

`wp-platform-driver-scaffold.md`；signal 设计文档与 **LoongArch 无关** 的纯数据结构设计。
