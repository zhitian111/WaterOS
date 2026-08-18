# wateros-fs-procfs-impl-kernel 离线开发手册

本 crate 将 task、cred、MM、网络、块设备和 VFS/IPC 回调的状态快照按需渲染为 Linux 风格
`/proc` 只读树。它拥有路径映射与格式，不拥有这些子系统的业务状态。公共契约见
[procfs-api](../../procfs-api/api-v0/README.md)，父级概览见 [fs-procfs](../../README.md)，
VFS 打开行为见 [impl-fs-bridge](../../../../wateros-vfs/vfs-impl/impl-fs-bridge/README.md)。

## 源码地图

| 文件 | 职责 |
| --- | --- |
| `src/path.rs` | 相对路径规范化、`ProcNode`/namespace 解析、稳定 inode 编码 |
| `src/view.rs` | `exists/metadata/read/read_symlink/read_dir` 的统一分派 |
| `src/render.rs` | task/MM/network/memory/mount 等 Linux 文本/二进制格式 |
| `src/callbacks.rs` | VFS、时间、SysV IPC 等跨组件函数指针注册表 |
| `src/fs_impl.rs` | procfs capability 占位和最小自检 |
| `src/lib.rs` | 模块组合与入口重导出 |

`KernelProcFs` 是零大小全局视图，`view()` 返回静态引用。可变状态只存在于 callback registry、
UUID sequence 和各业务子系统。

## 路径模型

`parse_node` 把规范化相对路径解析成 `ProcNode`。枚举覆盖：

- 根静态文件：meminfo、cpuinfo、stat、loadavg、uptime、mounts、devices 等；
- `/proc/net` socket/route/dev/sockstat；
- pressure、sysvipc；
- `/proc/sys/{kernel,vm,fs,net}` 的兼容只读值；
- `self`、`thread-self`；
- PID 的 stat/status/maps/smaps/cmdline/environ/auxv/io/fd/ns/task 等。

`normalize_rel` 接受带/不带 `/` 的相对输入。解析新增节点时必须避免数字 PID、`self`、静态
目录名之间的歧义；不能使用字符串前缀匹配把 `/proc/12x` 当 PID。

`proc_inode` 为静态节点分配固定小号，为 PID/task/fd 节点编码身份，namespace 使用固定的
Linux 初始 namespace 常见 inode。当前所有进程看到的是同一组 namespace identity；这些
magic link 不证明已经实现 `CLONE_NEW*`/`unshare` 隔离。

## view 的一致性矩阵

每个 `ProcNode` 必须同时出现在适用的分支：

```text
parse_node
→ exists（含 PID/fd/task 当前可见性）
→ metadata（File/Directory/Symlink、mode、inode、size）
→ read 或 read_symlink 或 read_dir
→ 父目录 read_dir 枚举
```

漏一个分支会产生典型错误：`ls` 能看到但 open ENOENT、stat 类型是 file 但 read 要求 symlink、
readlink 成功但 namespace open 无法转为 magic-link handle。

PID 存在性用 `task::process_snapshot(pid)`；fd 项通过 leader task + callback 的 fd Vec 验证；
thread task 节点还需验证 task 属于指定 process。进程在步骤间退出时返回 NotFound，不持有
registry 借用等待用户 copy。

## 跨组件回调与锁边界

每个回调保存为：

```text
Mutex<Option<fn(...) -> owned value>>
```

query helper 的正确模式是：

```rust
let lookup = *LOOKUP.lock(); // Copy 函数指针，guard 随语句释放
lookup.and_then(|f| f(id))   // 解锁后进入外部组件
```

这样避免 callback registry 锁与 VFS fd/mount/task/IPC 锁形成 ABBA。注册通常发生在启动阶段；
重新注册只是替换函数指针，没有 reader generation 或注销同步，函数必须是 `'static` 普通 fn
而非捕获闭包。

当前注册来源：

```text
user_bringup_bus:
  uptime、idle、timer slack、sysvipc
vfs::mount[_bootstrap]_procfs_at:
  argv/env/auxv/io/exe/cwd/root/fd/fd-target/mount-list
```

因此 mount procfs 之前直接读取某些节点可能得到空/0/NotFound。新增依赖时要把 register 放在
状态服务初始化之后、用户可见 mount 之前。

## 打开与读取调用链

```text
用户 open/read /proc/path
→ mount table 识别 PseudoViewKind::Proc
→ open_proc_node
→ view.metadata(rel)
├─ Directory：创建 ProcDirectoryHandle，首次 getdents 缓存目录项 Vec
├─ Symlink：readlink 走 view.read_symlink
├─ namespace magic link：VFS 转成特殊 file handle，保留 namespace kind/inode
└─ File：view.read(rel) 完整生成 Vec，存入 Arc
→ ProcFileHandle 按 open-description offset 从缓存 Vec 读取
→ syscall copy_to_user
```

关键点：普通文件是在 open 时生成一次完整快照，而不是每次 read 重新渲染。因此：

- 同一 open file description（dup/fork 共享 offset）内容保持稳定；
- 重新 open 才看到新 task/memory/network 状态；
- `random/uuid` 每次 open 生成新值，同一 fd 分段读不会改变；
- 大 maps/smaps 会在 open 时分配完整 Vec；
- view trait 的默认 `read_range` 并不是普通 VFS read 的当前热路径。

目录 handle 也在首次 getdents 时缓存 entry Vec，并用 entry index 作为 `d_off` cookie；之后 PID
退出不修改这次枚举，但打开对应项可以 NotFound。

## 渲染数据源与单位

| 节点族 | 主要数据源 | 注意事项 |
| --- | --- | --- |
| meminfo/vmstat | `mm_frame_alloctor::frame_mem_stats` | 物理页总量/空闲，不是固定 kernel heap |
| PID maps/smaps/statm | MM user mapping snapshot | 地址/权限/shared/private/file offset 与页/kB换算 |
| stat/status/sched/wchan | task/process/thread snapshot、cred | comm 括号、state 字符、tick 和 capability 宽度 |
| cmdline/environ/auxv | VFS exec 保存回调 | NUL 分隔；auxv 原始二进制 |
| fd/fdinfo | VFS fd callbacks | leader、offset/flags/target 与退出竞态 |
| mounts/mountinfo | mount table callback | namespace 快照、RO 标志、路径转义 |
| net tables | network socket snapshots | IPv4 proc 端序、state code、端口十六进制 |
| sysvipc | IPC registry callback | 表头/列必须与工具解析预期一致 |
| uptime/stat idle | platform timer/task idle ticks | ns 到秒/tick 的饱和换算 |

`format_meminfo()` 当前只输出 MemTotal/MemFree/MemAvailable 和零 Buffers/Cached。压力测试中看到
约 8 GiB 的 MemTotal 表示 frame allocator 可管理物理内存，不代表 512 MiB kernel heap 还有
同等空间；排查 OOM 必须同时看 heap 统计。

一些 `/proc/sys` 值是兼容常量而非真实可配置状态，且整个 procfs 只读。不要在文档或工具输出
中把 `pid_max`、`somaxconn`、overcommit 等常量误称为已实现 sysctl。
`/proc/sys/kernel/core_pattern` 同样是只读兼容值：它供 libc/LTP 判断 core-dump ABI，但 WaterOS
当前不会按该路径生成 core 文件；wait4 的 WCOREDUMP 与 waitid 的 CLD_DUMPED 只反映信号默认动作和
RLIMIT_CORE 条件。

## namespace magic link

`/proc/PID/ns/*` 在 procfs metadata/readlink 层表现为 symlink，目标类似 `mnt:[inode]`。VFS
open 时识别白名单路径，把它转换为只读特殊 file handle，使 `fstat` 保留 namespace inode，
供 `setns/ioctl` 兼容路径使用。普通 `exe/cwd/root/fd/N` symlink 不能被误转成 namespace handle。

当前 `PidForChildren` 与 `Pid`、`TimeForChildren` 与 `Time` 共用 inode，所有进程也共享全局
identity；实现真实 namespace 后必须从 task namespace state 取得实例 inode，并贯穿 clone、
unshare、setns 与 proc visibility。

## 随机 UUID 的安全边界

`/proc/sys/kernel/random/uuid` 使用 task tick + 原子 sequence 经 SplitMix64 生成格式正确的 v4
UUID。它保证同次启动内通常不同，但没有内核熵池，不具密码学随机性。不能用于密钥、token、
ASLR entropy 或安全随机 API；实现 `/dev/urandom/getrandom` 时必须走独立熵源。

## 新增节点的完整实例

以 `/proc/PID/foo` 为例：

1. 为状态所有者定义 owned snapshot callback，注册表遵守“复制 fn 后解锁再调用”。
2. `ProcNode` 增加 variant，`parse_node` 添加严格路径形状。
3. `proc_inode` 选择与其它 PID 子节点不冲突的编码。
4. `exists` 验证 process；`metadata` 指定 File、mode 0444、size 策略。
5. `render.rs::format_foo` 获取一次 snapshot，解锁后格式化并以换行结束。
6. `read` 分派；PID 目录的 `read_dir` 加 entry。
7. 在 owner 初始化之后、proc mount 之前注册 callback。
8. 用同 fd 小块读取验证快照/offset，用重新 open 验证刷新，并并发 exit PID。

若内容可能达到 MiB，评估 open-time Vec 的堆占用。可设计 chunked snapshot/stream handle，但需
保证一个 open description 的 offset 与内容版本一致，不能每块从变化中的状态重新拼接。

## `FsImpl` 注册项的特殊性

`KernelProcFsImpl` 声明 `(Other("procfs"), ReadOnly)` 仅用于 supported-fs 展示；
`mount_ro(block_device)` 返回 Unsupported。实际挂载由 VFS mount table 创建 pseudo view，不
经过块设备或 rootfs backend。不要把 capability 表理解为可用 `FsImpl::mount_ro` 挂盘。

## 故障定位

| 现象 | 首查 |
| --- | --- |
| `/proc/PID` 偶发 ENOENT | 进程退出竞态是否正常；是否保存了过期 PID/raw task 引用 |
| 文件存在但 cat 报类型错误 | view 的 metadata/read/readlink/readdir 分支是否一致 |
| 节点一直为空/0 | 对应 callback 是否在 proc mount 前注册 |
| proc read 小块时内容跳变 | 是否绕过 VFS open cache 直接重复调用 view.read_range |
| 同一个 fd 看不到最新状态 | 当前是 open-time snapshot，重新 open 才刷新 |
| maps/smaps 导致 heap 压力 | open 时完整 Vec；检查映射数与 formatter 分配 |
| `free` 显示 8G 但 heap OOM | meminfo 是 frame memory，不是 runtime heap cap |
| namespace readlink 能读但 open/ioctl 失败 | VFS magic-link 白名单与 inode/kind 转换 |
| ps/ss 解析失败 | 字段顺序、单位、括号、端序、换行是否符合实际工具 |
| procfs 与 VFS 死锁 | callback registry guard 是否跨外部调用，owner 是否持锁格式化 |

## 修改检查清单

- [ ] `ProcNode` 的 parse/inode/exists/metadata/read-or-link-or-dir/parent enumeration 全覆盖。
- [ ] callback 返回 owned snapshot，调用前已释放 registry 锁。
- [ ] task/MM/fd/mount/socket 业务锁不跨格式化和用户 copy。
- [ ] 动态退出/关闭导致 NotFound，而不是 panic 或 use-after-free。
- [ ] open-time snapshot、dup offset、reopen refresh 语义有测试。
- [ ] mem/page/tick/ns/kB 等单位与 Linux 工具预期一致。
- [ ] 大文件分配有上限或压力回归。
- [ ] 兼容常量没有被描述成真实可写 sysctl。
- [ ] UUID 等伪随机输出没有用于安全用途。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

guest 中建议至少执行：

```sh
cat /proc/meminfo /proc/uptime /proc/self/status
readlink /proc/self/exe /proc/self/ns/mnt
cat /proc/self/maps /proc/self/fdinfo/0
ls /proc/self/task /proc/self/fd /proc/net
ps
mount
```

再用后台快速创建/退出进程并发遍历 `/proc`，验证只出现可接受的 ENOENT，而无 panic、死锁或
堆持续增长。
