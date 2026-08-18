# wateros-fs-procfs-api-v0 离线开发手册

本 crate 定义 procfs 的版本化只读视图和跨组件快照回调类型。它不依赖 task、MM、VFS、IPC
或网络的具体实现；状态所有者通过函数指针向 procfs 提供值类型快照。内核实现见
[procfs impl-kernel](../../procfs-impl/impl-kernel/README.md)，父级概览见
[fs-procfs](../../README.md)，VFS 伪文件句柄见
[impl-fs-bridge](../../../../wateros-vfs/vfs-impl/impl-fs-bridge/README.md)。

## API 边界

```text
task/MM/VFS/network/IPC/platform（拥有状态）
             ↓ 注册 fn(TaskId) -> owned snapshot
procfs impl（路径与 Linux 文本格式）
             ↓ ProcFsView
VFS proc handle（open snapshot、offset、getdents、readlink）
             ↓
syscall 用户复制与 errno
```

API 的 `TaskId = usize` 数值与 task crate 一致，但故意不依赖 task 类型。回调不得返回锁 guard、
裸 task 指针或借用切片；`String`、`Vec`、数组等 owned snapshot 使 procfs 能在状态锁释放后格式化。

## 数据与回调

`ProcMountLine` 是 `/proc/mounts` 的中间值：device、mount point、fstype、readonly。renderer
负责转义、列顺序和 mount options；callback 不应直接拼一整张表，除非格式本身属于状态
子系统的 ABI（当前 SysV IPC 表就是 `Vec<u8>`）。

回调按来源分组：

| 状态所有者 | 回调 | 生成节点 |
| --- | --- | --- |
| VFS exec/cwd | argv/env/auxv/exe/cwd/root | `cmdline`、`environ`、`auxv`、`exe/cwd/root` |
| VFS fd table | fd list、fd target | `/proc/PID/fd`、readlink、fdinfo |
| task/syscall | I/O counters、timer slack | `io`、`timerslack_ns` |
| VFS mount table | `MountListLookup` | mounts/mountinfo |
| platform/task time | uptime、idle time | `/proc/uptime`、global stat |
| SysV IPC registries | `SysVIpcTableLookup` | `/proc/sysvipc/{shm,msg,sem}` |

回调的缺失语义由实现定义：部分返回 `None→NotFound`，部分回退空 Vec 或 0。新增回调必须在
API 注释和实现手册中明确“未注册”到底表示节点不存在、空内容还是零值，不能让工具把初始化
缺失误判成真实系统状态。

`SysVIpcTable` 是闭集枚举。增加新的表类型需要同步 callback 生产者、路径解析、目录枚举、
metadata/read 和测试。

## `ProcFsView` 契约

所有 `rel_path` 都相对 `/proc`，实现允许带或不带前导 `/`。各方法必须对同一节点类型保持
一致：

| 方法 | 要求 |
| --- | --- |
| `exists` | 未知/已退出 PID 返回 `Ok(false)`，不应 panic |
| `metadata` | 文件/目录/symlink 类型、mode、inode 稳定；不存在 `NotFound` |
| `read` | 生成一份完整普通文件内容；目录/symlink 返回 `NotAFile` |
| `read_range` | offset 超过 EOF 返回 0；默认实现会完整 `read` 后切片 |
| `read_symlink` | 只处理 symlink/magic link，目标不含尾 NUL |
| `read_dir` | 只处理目录；动态 PID/fd/task 项来自一次快照 |

默认 `read_range` 每次都会重新生成并分配完整 Vec。对于 maps、smaps、mounts、sysvipc 等大
文件，这在频繁小读下成本高，而且不同调用可能拼出不同时间点。当前 VFS 普通 proc handle
在 open 时完整 `read()` 并缓存内容，所以同一 fd 后续分段 read 保持一致；直接调用 view 的
消费者则没有这一保证。

## 动态快照与竞态

进程可能在 `exists → metadata → open/read` 任意阶段退出。正确语义：

- 查询前已不存在：`NotFound`/false；
- 已复制完整快照后退出：可以返回该自洽快照；
- 不能保存跨锁裸引用后继续 format；
- PID 重用时需要以 task/process registry 当前身份重新验证，不能缓存永久裸 PID 对象。

目录枚举与随后打开某项也允许竞态：`readdir` 看见 PID/fd 后，open 返回 ENOENT 是正常的
Linux 风格行为。不要为了“稳定”而在 proc dir handle 生命周期内持 task/fd table 锁。

## 新增节点实例：`/proc/PID/foo`

1. 在状态所有者定义最小 snapshot，例如：

   ```rust
   pub type TaskFooLookup = fn(TaskId) -> Option<[u64; 2]>;
   ```

2. 在 API 导出类型；不要让签名依赖 task 内部结构或 mutex guard。
3. 实现层添加 `Mutex<Option<TaskFooLookup>>`、register 和 query helper；query 先复制 fn，解锁
   后再调用。
4. `path.rs` 增加 `ProcNode::PidFoo(pid)` 与解析、稳定 inode 规则。
5. `view.rs` 同步 `exists`、metadata、read、readlink/read_dir 的类型分支。
6. `render.rs` 从 snapshot 生成 Linux 期望的单位、字段顺序和尾换行。
7. VFS/状态初始化在 mount procfs 前注册 callback。
8. 测试 PID 不存在、读取中退出、未注册 callback、offset/EOF/短缓冲、同 fd 一致快照和重新
   open 刷新。

若节点可写（如真正的 `/proc/sys` sysctl），不要扩展 `ProcFsView` 的 read 方法硬塞写副作用。
应设计带权限、解析、验证和原子更新语义的版本化写接口，并让 VFS/syscall 执行权限与用户
copy；当前 procfs 实现整体是只读的。

## ABI 格式检查

proc 文本是事实上的用户 ABI。新增或修复时逐项核对：

- 单位（页、字节、kB、tick、ns）和取整；
- 字段顺序、空格、冒号、括号与十六进制端序；
- 每行尾换行，以及 cmdline/environ 的 NUL 分隔；
- auxv 是本机字宽二进制，不是文本；
- symlink target 不追加 NUL；
- inode 在同一 namespace/进程生命周期内稳定；
- 敏感信息的可见性/权限（当前 API 不负责权限，必须由 VFS 补足）。

## 修改检查清单

- [ ] 回调返回 owned snapshot，不泄露外部锁或裸引用。
- [ ] query helper 在调用外部函数前释放 callback registry 锁。
- [ ] 未注册回调的语义明确且有测试。
- [ ] path/exists/metadata/read/readlink/readdir 同步新增节点。
- [ ] 动态进程退出、PID/fd 消失是可恢复错误。
- [ ] 大内容与 offset 语义考虑 open-time cache 和直接 view 两类调用者。
- [ ] Linux 格式的单位、NUL、换行、inode 和字段顺序已验证。
- [ ] 写节点没有绕过 VFS 权限与 syscall 用户复制。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

运行期至少使用 `cat`, `readlink`, `stat`, `ls`, `ps`, `mount`, `free`, `ip/ss` 等真实工具，
并验证 seek/短 read/重复 open。工具能解析比“文件能 cat”更能证明 ABI 格式正确。
