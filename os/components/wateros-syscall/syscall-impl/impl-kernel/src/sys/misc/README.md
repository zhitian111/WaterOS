# 系统控制与杂项 syscall 开发手册

[返回 impl-kernel](../../../README.md)

这里容纳无法归入单一对象域的系统控制 ABI。文件虽在同一目录，但状态所有者不同；修改前必须先沿下表
找到真正后端，不能把所有功能都做成 misc 全局变量。

## 代码地图

| 文件 | 状态/后端 | 关键规则 |
| --- | --- | --- |
| `sysinfo.rs` | `UTS_IDENTITY`、frame/task 统计、伪随机状态 | UTS 字段 65 字节；sysinfo 使用真实可得统计 |
| `syslog.rs` | wateros-klog reader | read/read-all/read-clear 的游标和用户复制 |
| `mount.rs` / `umount2.rs` | VFS mount namespace | source/target/fstype、传播 flag、root 权限 |
| `sync.rs` | VFS writeback、FS sync、block flush | fsync 单 fd，sync 全局，错误层次不能混淆 |
| `ioctl.rs` | TTY/RTC/fbdev/evdev/namespace fd 分发 | request 编码、fd 类型、结构布局 |
| `reboot.rs` | platform reset | 两个 magic、命令和权限全部通过后才执行 |
| `acct.rs` | `ACCOUNTING_PATH` | exit 记账文件路径和 64 字节 Linux acct 记录 |
| `riscv_*` | RISC-V ISA 特有 ABI | 非 RISC-V 不应伪造能力 |
| `bringup_stats.rs` | 诊断计数 | 仅可观测性，不是功能状态真相 |

## sysinfo/uname 链

```mermaid
flowchart LR
    A[sys_uname/sys_sysinfo] --> B[在内核构造 repr(C) 快照]
    B --> C[UTS mutex / wall clock / frame allocator / process registry]
    C --> D[饱和换算字段和单位]
    D --> E[一次 copy_to_user_struct]
```

不要在持 `UTS_IDENTITY` 锁时做用户复制。统计缺失时用文档化的零/近似值，但不得让 `MemFree` 大于
`MemTotal` 或把内核堆剩余量当成所有 guest RAM。

## ioctl 分发原则

`sys_ioctl` 先取得 fd/handle 类型，再按 request 分发到 TTY、RTC、framebuffer、input 或通用 FIONBIO。
增加 request 时确认 `_IOC` 方向/大小、结构 `repr(C)` 和目标 fd 类型。错误 fd 返回 `EBADF`，不支持该
对象的 request 返回 `ENOTTY`，坏用户地址返回 `EFAULT`。不要在大 match 中直接操作驱动私有锁。

## sync/writeback 边界

- `writeback`：把 VFS 页缓存脏页提交给文件系统；munmap/close 等可能需要。
- `fsync/fdatasync`：指定打开文件并要求后端同步。
- `sync/syncfs`：文件系统级边界，最终可能触发块设备 flush。

块设备不支持 flush 返回 `EIO` 时应在 driver/FS 层定位；不能让每次地址空间销毁隐式执行全 FS sync，
也不能把显式 fsync 改成无条件成功。

## 权限、架构与限制

hostname/domainname、mount、reboot、set-time 等状态修改当前部分以 root/effective capability 近似检查。
新增操作应尽量使用具体 capability。`getrandom` 当前缺少经审计硬件熵源/DRBG，不能用于宣称密码学安全。
module loading、swap 和完整 namespace 控制保持明确 `ENOSYS/EOPNOTSUPP`。

回归应覆盖 UTS 长度/并发、sysinfo 数值不变量、syslog 坏指针不推进游标、mount namespace fork/unshare、
fsync 错误传播、每类 ioctl 的错误 fd，以及 reboot magic 拒绝路径（不要在正常回归中真的重启宿主）。
