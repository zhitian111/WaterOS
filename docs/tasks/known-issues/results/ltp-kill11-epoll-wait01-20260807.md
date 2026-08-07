# LTP kill11 / epoll_wait01 修复结果

## 范围

定向 LTP 回归中两个真实失败点：

- `kill11`：`WCOREDUMP` 位错误地为不产生 core 的信号置位。
- `epoll_wait01`：`epoll_wait` 返回多个事件时第二个事件丢失。

## 根因

### kill11

`signal_terminate_exit_code()` 只检查 `RLIMIT_CORE > 0`，因此把
`SIGHUP/SIGINT/SIGKILL/SIGUSR1/SIGPIPE` 等也标记为 core dump。Linux 只对
`SIGQUIT/SIGILL/SIGTRAP/SIGABRT/SIGBUS/SIGFPE/SIGSEGV/SIGXCPU/SIGXFSZ/SIGSYS`
设置该位。

### epoll_wait01

内核此前按 12 字节 packed `epoll_event` 读写用户内存。当前 RISC-V/镜像中的
glibc 使用 16 字节布局：`events` 4 字节、4 字节 padding、`data` 8 字节。内核
连续写入两个事件时，第二个事件落在第一个 `data` 中间，导致用户态看到
`fd=0 events=0`。

## 改动

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`：
  按 Linux 信号表决定 `WCOREDUMP`。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/epoll_fd.rs`：
  RISC-V/LoongArch 使用 16 字节 `epoll_event` ABI，并保留 12 字节路径供宿主测试。

## 验证

- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V QEMU 单独运行 `kill11`：全部 TPASS，`KILL11_RC=0`
- RISC-V QEMU 单独运行 `epoll_wait01`：全部 TPASS，`EPOLL_WAIT01_RC=0`

## 剩余

`sigtimedwait01` 的 `siginfo` 与已退出子进程 `kill` 已部分改善，但
`test_bad_address2` 仍在无效 `sigset_t` 指针的用户拷贝路径阻塞，尚未闭环。
