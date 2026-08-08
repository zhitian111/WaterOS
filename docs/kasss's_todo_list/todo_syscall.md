
Optimized tool selection继续。我先把 busybox 路线图里已有的系统调用缺口分析与 busybox 源码实际用到的系统调用结合起来，做一次系统性的交叉核对。

Read [](file:///Users/x/code/WaterOS/docs/roadmap/riscv64-busybox/busybox-phased-plan.md#1-1), lines 1 to 120

让我读取 busybox 配置中启用的完整 applet 列表，再针对 WaterOS 未分发的系统调用在 busybox 源码中定位使用点。

Ran terminal command: grep -E '^CONFIG_[A-Z0-9_]+=y$' /Users/x/code/WaterOS/test_case/config/busybox-config-riscv64 | grep -vE 'FEATURE_' | sed 's/CONFIG_//; s/=y//' | tr '\n' ' '

现在对 WaterOS 未分发的候选系统调用，在 busybox 源码里确认实际使用位置（哪个 applet、什么用法）：

Ran terminal command: for sc in sethostname setdomainname reboot swapon swapoff init_module delete_module finit_module ioprio_set ioprio_get setns pivot_root chroot personality readahead syncfs copy_file_range membarrier getcpu timerfd_create signalfd inotify quotactl gettid; do echo "== $sc =="; grep -rn -E "\b$sc\b" --include='*.c' --include='*.h' . 2>/dev/null | grep -v testsuite | head -5; done 2>&1 | head -120

再确认几个关键点（splice/tee、SysV 消息/信号量、以及相关 applet 是否在配置中启用）：

Ran terminal command: for sc in splice vmsplice tee ptrace msgget semget semctl msgctl semop getdomainname gethostname; do echo "== $sc =="; grep -rn -E "\b$sc\b" --include='*.c' . 2>/dev/null | grep -v testsuite | head -4; done; echo "== config check =="; for a in INOTIFYD READAHEAD IPCALC IPCS IPCRM QUOTA REBOOT HALT POWEROFF; do c=$(grep -E "^CONFIG_${a}=y" /Users/x/code/WaterOS/test_case/config/busybox-config-riscv64); echo "$a: ${c:-NOT-ENABLED}"; done

Ran terminal command: echo "== sync.c config/usage =="; sed -n '10,30p;55,75p' coreutils/sync.c; echo; echo "== reboot.c syscall usage =="; grep -n -E "reboot|syscall" init/reboot.c | head; echo; echo "== config flags =="; grep -E "FEATURE_SYNC_FANCY|^CONFIG_CHROOT=|^CONFIG_READAHEAD=|^CONFIG_HOSTNAME=|^CONFIG_DNSDOMAINNAME=" /Users/x/code/WaterOS/test_case/config/busybox-config-riscv64

Ran terminal command: echo "== sync.c config/usage =="; sed -n '10,30p;55,75p' coreutils/sync.c; echo; echo "== reboot.c syscall usage =="; grep -n -E "reboot|syscall" init/reboot.c | head; echo; echo "== config flags =="; grep -E "FEATURE_SYNC_FANCY|^CONFIG_CHROOT=|^CONFIG_READAHEAD=|^CONFIG_HOSTNAME=|^CONFIG_DNSDOMAINNAME=" /Users/x/code/WaterOS/test_case/config/busybox-config-riscv64

Ran terminal command: echo "== readahead.c =="; grep -n -E "readahead|syscall" util-linux/readahead.c | head; echo "== halt.c (reboot) =="; grep -n -E "reboot\(|syscall|RB_|kexec" init/halt.c | head; echo "== chroot.c syscall =="; grep -n -E "chroot\(|syscall" coreutils/chroot.c | head; echo "== other missing-sc check =="; for sc in sync_file_range vhangup kexec_load ptrace; do echo "-- $sc --"; grep -rn -E "\b$sc\b" --include='*.c' . 2>/dev/null | grep -v testsuite | head -3; done

Ran terminal command: echo "== readahead.c =="; grep -n -E "readahead|syscall" util-linux/readahead.c | head; echo "== halt.c (reboot) =="; grep -n -E "reboot\(|syscall|RB_|kexec" init/halt.c | head; echo "== chroot.c syscall =="; grep -n -E "chroot\(|syscall" coreutils/chroot.c | head; echo "== other missing-sc check =="; for sc in sync_file_range vhangup kexec_load ptrace; do echo "-- $sc --"; grep -rn -E "\b$sc\b" --include='*.c' . 2>/dev/null | grep -v testsuite | head -3; done

Ran terminal command: f=$(find . -name readahead.c | head -1); echo "file: $f"; grep -n -E "readahead\(|syscall|READAHEAD" "$f" | head; echo "== xchroot in libbb =="; grep -rn -E "xchroot|chroot\(" libbb/*.c | head

最后核对一下 WaterOS 是否在旁路（alias）路径处理了这些号：

Searched for regex `dispatch_syscall_aliases|SETHOSTNAME|REBOOT|SWAPON|SWAPOFF|INIT_MODULE|DELETE_MODULE|FINIT_MODULE|IOPRIO|SETNS|PIVOT_ROOT|CHROOT|PERSONALITY|SYNCFS|READAHEAD|MSGGET|SEMGET|SEMOP` (`**/os/components/wateros-syscall/**`), 1 result

核对完毕。所有候选系统调用在 WaterOS 中既未定义在 number.rs，也未在 syscall_nr_dispatch.rs 分发（全部落入 `_ => ENOSYS` 兜底分支）。我把结论整理成文并记入仓库备忘。

Created memory file [](file:///Users/x/Library/Application%20Support/Code/User/workspaceStorage/d47527aee285c4c168070621db9e228e/GitHub.copilot-chat/memory-tool/memories/repo/wateros-busybox-missing-syscalls.md)

检查整理完成。结论：**当前 WaterOS 已实现并分发 140+ 个系统调用**，但对照 busybox 1.33.1（busybox-config-riscv64 启用的 applet）逐一定位，仍有 **8 类、约 20 个系统调用** 被 busybox 命令真实调用，而 WaterOS 完全未实现（未在 number.rs 定义、未在 syscall_nr_dispatch.rs 分发，全部落入 `_ => ENOSYS` 兜底）。

## 缺失系统调用清单（busybox 用到 → WaterOS 返回 ENOSYS）

### 🟥 进程/文件系统类（普通命令就能触发，优先级高）


| syscall (asm-generic nr) | 使用它的 busybox applet                 | 触发方式                                                    |
| -------------------------- | ----------------------------------------- | ------------------------------------------------------------- |
| `chroot` (51)            | `chroot`                                | `chroot dir cmd`（`libbb/xfuncs_printf.c::xchroot`）        |
| `pivot_root` (41)        | `pivot_root`、`switch_root`、`run-init` | 切换根文件系统                                              |
| `sethostname` (161)      | `hostname`                              | `hostname <新名>`（`networking/hostname.c`）                |
| `setdomainname` (162)    | `hostname -d` / `dnsdomainname`         | 设置域名                                                    |
| `syncfs` (251)           | `sync -f FILE`                          | 配置已开`CONFIG_FEATURE_SYNC_FANCY=y`（`coreutils/sync.c`） |
| `readahead` (213)        | `readahead`                             | `readahead file`（`miscutils/readahead.c`）                 |

### 🟨 电源/交换区/调度（对应 applet 一运行就失败）


| syscall                               | 使用它的 applet                 | 触发方式                                    |
| --------------------------------------- | --------------------------------- | --------------------------------------------- |
| `reboot` (142)                        | `reboot`、`halt`、`poweroff`    | 关机/重启（`init/halt.c:243`）              |
| `swapon` (224) / `swapoff` (225)      | `swapon`、`swapoff`             | 启用/停用交换区（`util-linux/swaponoff.c`） |
| `ioprio_set` (30) / `ioprio_get` (31) | `ionice`                        | 设置 IO 优先级（`util-linux/ionice.c`）     |
| `personality` (92)                    | `setarch`、`linux32`、`linux64` | 切换执行域（`util-linux/setarch.c`）        |

### 🟦 模块 / 命名空间 / SysV IPC（需要对应内核子系统）


| syscall                                                            | 使用它的 applet                                             | 说明                                                                         |
| -------------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `init_module` (105) / `finit_module` (257) / `delete_module` (106) | `insmod`、`rmmod`、`lsmod`、`modinfo`、`modprobe`、`depmod` | 模块装载（`modutils/modutils.c` 直接 `syscall()`）                           |
| `setns` (253)                                                      | `nsenter`                                                   | 进入命名空间（`util-linux/nsenter.c`）                                       |
| `msgget/msgctl/msgrcv/msgsnd` (186–189)                           | `ipcs`、`ipcrm`                                             | SysV 消息队列                                                                |
| `semget/semctl/semop` (190–193)                                   | `ipcs`、`ipcrm`、**`syslogd`、`logread`**                   | SysV 信号量（`syslogd`/`logread` 靠 `semget`+`semop` 做 IPC 同步，比较隐蔽） |

## 补充说明

- **当前行为**：以上全部走 `dispatch_syscall_by_nr` 的兜底分支 `_ => UserRet::from_error(ENOSYS)`，不会再 panic；用户态 busybox 会报 `Function not implemented`。
- **优先级建议**：`chroot` / `pivot_root` / `sethostname` / `reboot` / `swapon` / `personality` / `syncfs` / `readahead` / `ionice` 这几个实现成本低（多为单函数桩 + 少量语义），对 busybox 命令可用性提升最直接；模块、`setns`、SysV msg/sem 需要对应内核子系统支撑，建议按 roadmap 排期。
- **另一个值得注意的点**：`rt_sigreturn`(139) 在分发表里目前硬编码返回 `ENOSYS`（实际由 trap 特殊路径处理）。信号量多的命令（如 `ash` 作业控制、`timeout`）是否受影响，取决于 trap 路径是否已接管——不在本次「缺失 syscall」范围内，但建议确认。

需要的话，我可以挑其中一两个（比如 `chroot` + `sethostname`）直接落地实现。
