# 决赛 CAgent 与 BuildStorm 运行阻断项

## 检查范围

检查日期：2026-07-27。测试环境为 RISC-V64、OpenSBI、QEMU `virt`、8 核、
8 GiB 内存和 `os/sdcard-rv-pub.img`。诊断使用 qcow2 overlay，没有修改基线镜像；
诊断结束后 `os/src/user_bringup_busybox.rs` 已恢复为依次运行：

```text
/usr/bin/busybox sh /glibc/cagent_testcode.sh
/usr/bin/busybox sh /glibc/buildstorm_testcode.sh
```

本文只记录已经由运行日志或源码确认的阻断，不把 syscall 已存在等同于测例可用。

## 已确认根因

### P0：普通 glibc 动态程序无法 exec

最小探针结果：

```text
/usr/bin/sleep 0       -> Permission denied, rc=126
/usr/bin/date +%s%3N   -> Permission denied, rc=126
/usr/bin/uname -m      -> Permission denied, rc=126
/glibc/busybox sleep 0 -> rc=0
```

这排除了调度器和 `nanosleep` 完全不可用。`execve` 将
`RootVolumeReadError::NotAFile` 映射为 `EACCES`。动态 ELF 的 `PT_INTERP` 指向
`/lib/ld-linux-riscv64-lp64d.so.1`，而镜像中的 `/lib` 是到 `usr/lib` 的符号链接，
动态链接器本身也可能是符号链接。当前 RISC-V loader 的解释器重映射只对
`/glibc/*` 或 `/musl/*` 主程序生效，不能正确装载 `/usr/bin/*` 和
`/root/.cargo/bin/*` 的 Debian 动态程序。

待办：

- [ ] VFS 路径读取和 `execve` 支持最终分量及中间分量符号链接，处理相对链接、绝对
      链接、最大递归深度和循环。
- [ ] ELF loader 按解析后的 `PT_INTERP` 装载真实动态链接器，不以主程序位于
      `/glibc` 为前提。
- [ ] 保证主程序、动态链接器和共享库的 auxv、load bias、TLS、`mmap`、`mprotect`
      语义满足 glibc。
- [ ] 回归 `/usr/bin/sleep`、`date`、`uname`、`bash`、`rustup`、`rustc` 和
      `cargo`，要求不再返回 126。

### P0：CAgent 使用了错误的 shell

`cagent_testcode.sh` 的 shebang 是 `/bin/bash`，当前 bringup 却强制使用 BusyBox
`sh`。BusyBox `date +%s%3N` 输出形如 `1785127208%3N`，随后脚本第 25 行执行
`$((end_time - start_time))`，十个后台任务都报 arithmetic syntax error。

待办：

- [ ] 动态 ELF 修复后，通过 shebang 或真实 `/usr/bin/bash` 启动 CAgent，移除
      `compat_exec_load_path()` 对 bash/dash 的 BusyBox 替换。
- [ ] 验证 GNU `date +%s%3N` 返回纯数字毫秒值。
- [ ] 不以修改 BusyBox `date` 作为最终方案；官方测例明确要求 glibc/bash 环境。

### P0：CAgent 脚本自身存在永久等待

脚本先执行 `./simple_llm_server 8080 &`，随后使用无参数 `wait` 等待所有后台任务，
但 `kill $SERVER_PID` 位于 `wait` 之后。`simple_llm_server` 的 accept 循环只有收到
SIGINT/SIGTERM 才退出，因此当前脚本在 Linux 语义下也会等待 server，无法进入清理。

待办：

- [ ] 向测例维护者确认脚本版本；推荐记录十个测试任务 PID，只等待这些 PID，再
      `kill` 并 `wait` server。
- [ ] 在修正版脚本上验证十项均结束并打印 GROUP END，不能用内核伪造退出规避脚本问题。

### P0：BuildStorm 环境检查尚未过门槛

最小探针确认：

```text
rustup --version -> rc=126
rustc --version  -> rc=126
cargo --version  -> rc=126
cat /proc/uptime -> No such file or directory
```

待办：

- [ ] 先完成动态链接器和符号链接路径修复，使工具链能启动。
- [ ] 实现 `/proc/uptime`，格式为 `<uptime_seconds> <idle_seconds>\n`，数据来自单调
      启动时钟；该文件是 BuildStorm 计时依据。
- [ ] 验证 `/proc/cpuinfo`、`sched_getaffinity` 和 `nproc` 一致报告 8 个 online CPU。
- [ ] 验证 `mount -t proc/sysfs/devtmpfs`、`/dev/null`、`/dev/urandom` 和 `/dev/tty`。
- [ ] 工具链通过后再运行 minibuild，并按首个失败 syscall 继续补齐
      fork/exec/wait、futex、pipe、poll/epoll、文件 mmap 和并发 ext4。

## 推荐执行顺序与验收

1. 修复通用 VFS 符号链接解析和动态 ELF `PT_INTERP`，跑通
   `/usr/bin/sleep`、`/usr/bin/bash`、`rustc --version`。
2. 将 CAgent 改为真实 bash 启动，并与测例维护者解决 server `wait` 问题。
3. 实现 `/proc/uptime` 和在线 CPU 报告，跑通 BuildStorm 工具链检查。
4. 运行 CAgent 十项并发测试；确认 GROUP END 后才判定“无卡死”。
5. 运行 cargo minibuild，最后进入 BuildStorm 全量编译和性能优化。

完成标准是日志同时出现 CAgent GROUP END、`BUILDSTORM_TOOLCHAIN ok` 和
`BUILDSTORM_MINIBUILD ok`；仅能启动脚本或 BusyBox 命令成功不能视为完成。
