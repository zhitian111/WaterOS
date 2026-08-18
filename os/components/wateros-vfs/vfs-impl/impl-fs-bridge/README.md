# FS Bridge 离线开发手册

[VFS 总览](../../README.md) · [VFS API](../../vfs-api/api-v0/README.md) · [FD Session](../impl-fd-session/README.md) · [页缓存](../impl-page-cache/README.md)

`FsBridge` 是 `wateros-vfs` 到 `wateros-fs` 的适配与路由层。它把用户可见绝对路径映射到根卷、辅助 RW/RO 卷、procfs、sysfs、securityfs 或 bind 源；把 FS 元数据/错误转换成 VFS 类型；为普通文件构造页缓存句柄；并维护 mount namespace、稳定 node lease 和 open-unlink/rename 后的句柄语义。

它不实现 Linux pathname walk、dirfd/cwd、fd table、权限判定、ext4 磁盘格式或块设备 I/O。相邻层归属错误是本模块最常见的维护问题。

## 1. 源码地图

| 文件 | 关键内容 |
|---|---|
| `lib.rs` | `FsBridge`、FS/VFS 类型转换、设备覆盖、open 分流、trait 实现 |
| `mount_table.rs` | `MountNamespace`、`MountEntry`、最长前缀/bind 路由、mount identity |
| `mount_ns.rs` | task→owner→`Arc<MountNamespace>` 注册表、共享/COW/unshare |
| `path_ops.rs` | mount、unlink、rename、truncate、link、xattr、mknod 等事务编排 |
| `paged_handle.rs` | 普通 RW 文件页缓存句柄、open ref、OFD offset、writeback |
| `stable_node.rs` | stable node lease、内容版本、detached state、rename/unlink 提交 |
| `file_handle.rs` | RO/兼容路径的全文缓冲句柄 |
| `read_lease.rs` | prepared-read 的 OFD offset reservation 与可失败 staging buffer |
| `dir_handle.rs` | 目录 snapshot、Linux `dirent64` 编码、目录 cookie |
| `proc_handle.rs` | proc/sys 只读 snapshot handle、namespace magic-link fd |
| `sysfs.rs` | 内核生成的最小 sysfs 视图 |
| `symlink_handle.rs` | `O_PATH|O_NOFOLLOW` 打开的链接节点句柄 |
| `tmpfs.rs` | cgroup 测试兼容用内存树；普通 tmpfs 实际来自 `fs::new_ramfs_rw` |

## 2. 分层与端到端链路

```text
Linux syscall handler
  -> VFS 聚合层：dirfd/cwd/root、symlink follow、权限、errno
  -> FsBridge：mount route、句柄类型、FS/VFS 转换
  -> page cache（普通 RW 文件）
  -> wateros-fs ReadWriteFs/ReadOnlyFs/ProcFsView
  -> ext4/ramfs/procfs/devfs
  -> block/driver
```

判断代码应放哪层：

- Linux flag、`AT_*`、用户指针、errno：syscall；
- cwd/root/dirfd、fd/OFD、pipe/设备句柄：VFS 聚合或 fd-session；
- mount 路由、stable-node 与 FS 错误映射：本 crate；
- inode/extent/目录块/journal：FS；
- 缓存 page/LRU/dirty/writeback 状态机：page-cache；
- VMA/file mapping：MM。

## 3. 挂载数据结构

### 3.1 `AuxMount` 与 `MountEntry`

`AuxMount` 当前有六种：

```text
Rw(SharedRwFs)       可写后端，可被 remount 标成 readonly
Ro(SharedFs)         底层本身只读
PseudoProc           动态 proc view
PseudoSys            内核生成 sys view
PseudoSecurity       当前仅有空的只读根目录
Bind { source }      路径别名，继续解析 source
```

每个 `MountEntry` 保存：

- `mount_point`：规范化绝对路径；
- `fs`：上述后端/别名；
- `identity`：device major/minor + 唯一 mount id；
- `readonly`：挂载级写保护；
- `fstype`：供 `/proc/mounts`/statfs；
- `propagation`：Private/Shared/Slave/Unbindable 标签。

根挂载固定 `mount_id=1`。辅助挂载从 2 单调分配；同一 `device_key` 复用 device minor，但每次 mount 都取得新 mount id。缓存和 `stat` 身份不能只看 inode，因为不同 mount 上 inode 值可相同。

### 3.2 路由结果 `FsRoute`

```text
Root       { abs, identity }
AuxRw      { fs, rel, identity, readonly }
AuxRo      { fs, rel, identity }
PseudoProc { rel, identity }
PseudoSys  { rel, identity }
PseudoSecurity { rel, identity }
```

`MountNamespace::longest_match` 只接受完整路径组件边界：挂载 `/mnt` 会匹配 `/mnt/a`，不会误匹配 `/mnt2`。没有条目时走根卷。

bind mount 把“bind 点下的相对部分”拼到 source，再重新做最长前缀解析；最多跟随 32 次，超过返回 `InvalidPath`，防止 bind 环或过深链。material route 最终返回源后端身份，不保留 bind entry 自身的 identity。

### 3.3 可写检查

任何会修改路径的操作必须先调用 `assert_path_writable`：

- `AuxRw(readonly=true)`、AuxRo、proc、sys、security 返回 `ReadOnlyFs`；
- Root 和正常 AuxRw 放行；
- `getxattr/listxattr` 是只读查询，必须走 `fs_and_rel_rw_query`，不能因 RW 后端被 remount-ro 而错误拒绝；
- 真正的 AuxRo 和 pseudo view 没有 RW xattr 接口，返回 `Unsupported`。

## 4. Mount namespace 生命周期

### 4.1 三张表

`PerTaskMountNsRegistry`：

```text
namespaces: owner TaskId -> Arc<MountNamespace>
owners:     member TaskId -> owner TaskId
ref_counts: owner TaskId -> member count
```

没有当前 task 的 early bring-up 使用 `BOOTSTRAP_MOUNT_NS`。新任务第一次初始化时继承 bootstrap 的 `Arc` 快照。

### 4.2 clone/fork/unshare

```text
普通 clone/fork（无 CLONE_NEWNS）
  -> share_mount_ns_from_parent
  -> child 指向 parent 的 owner
  -> mount 修改对组内所有成员可见

CLONE_NEWNS
  -> copy_mount_ns_from_parent
  -> child 有自己的 owner，但先 Arc 共享只读 snapshot
  -> 首次修改 namespace_for_mut -> Arc::make_mut -> COW

unshare(CLONE_NEWNS)
  -> 若 refcount>1，复制 Arc snapshot 并拆出 caller
  -> 如果 caller 恰是 owner，先把其余成员 re-home 到另一 member

task reap/exit cleanup
  -> drop_task_mount_ns
  -> refcount--，最后成员释放 namespace slot
```

注意：源码中 copy 的注释容易让人误读；实际 syscall 调用方已经明确 `CLONE_NEWNS -> copy`，无该 flag -> share。

### 4.3 初始化与锁

registry 使用 `MultiprocessorSafeCell` 包住整张注册表。路由会先克隆 `Arc<MountNamespace>` snapshot，再释放 registry 锁，避免持全局锁调用 FS。

当前 `registry()` 用 `static mut MaybeUninit + READY Atomic` 惰性初始化，没有 once 状态机；必须保证首次调用在串行 bring-up 完成。若多个 CPU 同时第一次进入，存在并发写静态存储的风险。扩展时优先改为项目统一的 once-cell，而不是继续增加裸原子分支。

## 5. 挂载操作的真实能力边界

支持的入口包括 ext4 block、ramfs tmpfs、bootstrap tmpfs、proc/sys/security、cgroup 模拟、bind/recursive bind、move、remount-ro 和 unmount。

当前限制必须明确：

- `MountPropagation` 目前只记录标签；没有 peer group/master，也没有把后续 mount/unmount 事件传播到 shared/slave；
- `Unbindable` 仅阻止其路径作为 bind source；
- `unmount_aux_at(..., detach)` 当前忽略 `detach` 参数；普通与 lazy detach 行为相同；
- 没有 mount busy/open-handle 检查，也没有递归卸载子挂载；
- `VfsMountTable::mount_at` 与 `resolve_mount` 仍返回 `Unsupported`，真实 syscall 使用 `path_ops` 的专用入口；
- 根挂载不能经此表卸载或移动；
- 跨 namespace 的 mount 修改仍会推进 rootfs 的全局 mount generation。

每次结构变化会先尝试 `reset_file_page_cache()`，再推进全局 mount generation。当前 flush 失败只记录 warning，随后仍 bump generation；这可能让旧 generation 的脏 cache 无法由新路径正常取得。若遇到 mount/unmount 后写回失败或内存滞留，首先检查这条日志。更严格的修复应让“缓存成功提交/隔离”和“generation 发布”成为可回滚事务。

## 6. 普通文件核心数据结构

### 6.1 `PagedFileHandle`

关键字段：

| 字段 | 语义 |
|---|---|
| `path` | 初始路径；rename 后实际路径从 detached state 取 |
| `description: Arc<VfsOpenDescriptionState>` | OFD 共享 offset/status flags/reservation |
| `meta` | 打开时元数据 fallback |
| `writable/accmode` | 访问能力 |
| `mount_gen` | 句柄打开时的 cache generation |
| `on_disk_size` | 打开/截断时已知后端大小 |
| `detached: Arc<Mutex<DetachedState>>` | 同一路径打开句柄共享的 rename/unlink 状态 |
| `open_ref_held` | 是否持 page-cache open reference |
| `anonymous/tmpfile_linkable` | `O_TMPFILE` 语义 |
| `tmpfile_linked_path` | `linkat(AT_EMPTY_PATH)` 后记录路径 |
| `flock_owner_id` | OFD/flock owner；duplicate 保持相同值 |

`Clone`（dup/fork 的底层 duplicate）共享 `description` 和 detached state，并增加 page-cache open ref，因此 offset 和 O_APPEND 状态按 OFD 共享。`tmpfile_linked_path` 当前复制值到新的 Mutex，而不是 Arc 共享；重复 fd 间对“后来 link 到哪个路径”的观察可能不同，扩展 `AT_EMPTY_PATH` 行为时要修正这一点。

### 6.2 `StableNodeLease`

RW FS 若支持 node API，open 时取得：

```text
SharedRwFs + FsNodeId + MountIdentity + VfsFileContentIdentity
```

cache key 为 `@node:<mount_id>:<node_id>`，不依赖 pathname。rename/unlink 后句柄仍读写同一个 node。最后一个 lease Drop 调用 `close_node`；失败只能记录 warning，因为 Drop 不能返回错误。

内容 identity 的 key 是 `(mount_generation, mount_id, node_id)`，共享 `AtomicU64` version。写、truncate、link/unlink 等内容/身份变化调用 `mark_changed`，供 mmap/exec 等消费者识别 stale 内容。

### 6.3 `DetachedState`

```text
detached  是否已与后端 pathname 脱离
path      rename 后的当前可见路径
data      无 stable-node 后端的 unlink 快照
stable    可选 StableNodeLease
cache_key stable key 或 path key
```

全局表保存 `Weak`，生命周期由打开句柄决定；周期性清理失效 weak stable 注册。无 stable-node 支持时，为维持 open-unlink 语义会把完整 logical file 复制到堆，硬上限 16 MiB；超过返回 `Io`。这不是普通页缓存容量，而是兼容 fallback。实现新 RW FS 时优先提供 node lease，避免大文件 unlink 受此限制。

## 7. open 分流

```text
FsBridge::open_path(path, flags)
  -> 绝对路径 normalize；相对路径由 VFS resolve_open_path
  -> resolve_route
  -> proc/sys：只读 pseudo handle
  -> securityfs：仅目录根
  -> 内建 /dev/null,/zero,/random,/urandom,cpu_dma_latency
  -> fd-session 注册的特殊设备
  -> devfs character device
  -> symlink：O_PATH|O_NOFOLLOW 场景的 SymlinkPathHandle
  -> FIFO special：fd-session named pipe
  -> 普通目录：DirectoryHandle
  -> AuxRo：BufferedFileHandle
  -> Root/AuxRw：PagedFileHandle
```

页缓存 open：

```text
metadata/CREATE
  -> open_stable_node（可能返回 None）
  -> detached_state_for_open
  -> 生成 FileCacheKey
  -> O_TRUNC：先后端 truncate，再 cache truncate
  -> O_APPEND：offset=cache logical size
  -> acquire_open_ref_key
  -> 返回句柄
```

当前 `FILE_IO_MODE::Async` 直接 `Unsupported`；只有 Direct 模式实现。AuxRo 使用全文缓冲句柄，写被拒绝。

## 8. 读、写、offset reservation

### 8.1 为什么需要 prepared read

用户 copy 可能只复制部分数据或 fault，不能在 staging 成功时就无条件推进共享 OFD offset：

```text
prepare_read(max_len)
  -> VfsOpenDescriptionState::begin_read，锁定 reservation(offset)
  -> acquire：从 detached/cache/backend 暂存到 fallible Vec
  -> user copy
  -> lease.finish({copied, complete})
       -> 只按 copied 推进 offset
       -> copied=0 且未 complete => Fault
  -> 任意提前 Drop => cancel_read，offset 不变
```

`try_zeroed` 使用 `try_reserve_exact`，OOM 映射 `NoMemory`。不要改回 `vec![0; len]` 等不可失败分配。reservation 存活期间 seek/并发 read 必须由 `VfsOpenDescriptionState` 的 busy 规则协调。

### 8.2 普通 read/write

- `read/write` 使用共享 OFD offset；`read_at/write_at` 不改 offset；
- O_APPEND 在每次 write reservation 后重新指向当前 logical EOF，而不是只在 open 时设置；
- 页缓存写扩大 logical size，metadata 覆盖缓存大小；
- stable node 后端 miss/flush 直接按 node id I/O；path key 后端 NotFound 时句柄切 detached fallback；
- O_SYNC 写后调用 `sync_dirty`；
- poll 对普通文件按请求返回 readable，writable 句柄返回 writable。

## 9. 写回、flush、close 与 Drop

语义区分：

```text
writeback()/close
  -> page-cache dirty pages 写到 FS 缓存/文件数据

flush()/O_SYNC
  -> writeback_dirty
  -> stable_node.sync 或 path 所属 FS sync

sync_file_page_cache
  -> 尽力 flush 全局 cache
  -> 忽略 Unsupported/ReadOnlyFs/NotFound
  -> root FS sync
```

显式 `close()` 返回 writeback 错误，随后无论成功失败都释放 open ref。`Drop` 也尝试 writeback，但只能记录 warning；如果 syscall/fd-session 没有先调用显式 close，持久化错误无法反馈用户。

`reset_file_page_cache` 只有在无活跃脏 fd 的安全点使用：先全局 sync，再重建空 cache。不能为了缓解 OOM 在任意时刻调用，否则活跃句柄仍持旧 generation/key。

锁序：

```text
page-cache files（短）
  -> per-file entry RwLock
  -> page-cache state（短）
  -> SharedRwFs（仅实际 range I/O）
```

真正 `read_range/write_range` 必须在 cache state 锁外。禁止持 ext4/FS 锁反向等待 entry 锁；否则 read miss 与 fsync/writeback 可死锁。

## 10. unlink 与 rename 事务

### 10.1 unlink

```text
assert writable
  -> prepare_unlink_detach(path)
       ├─ stable node：mark version，暂不复制数据
       └─ path-only：从 cache/backend 复制完整 logical data（<=16 MiB）
  -> backend unlink
  -> 仅成功时 PendingUnlinkDetach::commit
       -> 从 path registry 移除
       -> path-only state 切 detached 并装入 snapshot
  -> purge_closed_file(path)
```

准备阶段不能先永久切断路径；否则 backend unlink 失败会让仍存在的文件与打开句柄分裂。目录删除不走 detached 文件逻辑。

### 10.2 rename

要求 old/new 落在同一个 `SharedRwFs`（`Arc::ptr_eq`），跨 mount 返回 `Unsupported`，由 syscall 映射 EXDEV 类语义。

```text
normalize + 两端 writable
  -> flush old/new path cache
  -> 若 target 存在，准备其 detach
  -> target rename 到唯一隐藏临时名
  -> source rename 到 target
       └─ 失败：临时名 rollback 到 target
  -> unlink/rmdir 临时 target
  -> commit_rename_state
       ├─ page cache finish_rename
       ├─ source detached state 改新路径
       └─ 被替换 target 句柄进入 detached/stable 状态
```

隐藏临时名最多尝试 64 次。若最后 cleanup 失败，源码记录 error 并返回失败，但 source 已经位于 new path，调用方不能假设“返回错误就完全没发生”。修复时需引入更强后端原子 rename/replace 契约或完整补偿状态。

## 11. 目录、proc/sys 与特殊节点

### 11.1 目录 handle

第一次 `getdents64` 时读取并缓存整个目录列表；之后是打开时段内的 snapshot，不会自动观察新增/删除。offset 是下一条 entry 的数组索引 cookie，duplicate/fork 共享 OFD state。记录布局：ino 8、d_off 8、reclen 2、type 1、NUL name，再向 8 字节对齐。

若 buffer 连下一条完整记录都装不下且目录未 EOF，返回 `InvalidPath`（上层应映射 EINVAL），游标不动；不能返回 0 冒充 EOF。当前 inode 字段统一编码为 1，不是后端真实 inode。

### 11.2 proc/sys snapshot

proc/sys 文件在 open 时调用 view.read 并存为 `Arc<Vec<u8>>`，同一 fd 后续 read 是 snapshot；重新 open 才刷新。目录同样首次 getdents 时 snapshot。

`/proc/<pid|self|thread-self>/ns/<kind>` 路径元数据仍是 magic symlink，但 open 后变成空的只读 regular-like namespace fd，并通过 `namespace_kind()` 保留类型；当前尚无完整 `setns(2)`。

sysfs 是手工生成的兼容视图，只覆盖源码中列出的 CPU、node、block 等路径，不是通用 kobject/sysfs 框架。securityfs 当前只有空、只读根目录。

### 11.3 `/dev` 覆盖

实体 devfs 目录项会与 fd-session 注册的特殊设备合并，并为 `/dev/input` 等中间目录合成 metadata。内建设备和动态设备优先于普通根卷同名文件。

`/glibc/sort.src` 与 `/musl/sort.src` 是 UnixBench 兼容的只读虚拟文件，仅在根后端报告 NotFound 时出现；不要把它当作通用 overlay FS。

## 12. cgroup/tmpfs 与 xattr

普通 `mount -t tmpfs` 调用 `fs::new_ramfs_rw(limit, mode)`；本 crate 的 `TmpFs` 主要是 cgroup v1/v2 测试兼容树，内含 file/dir/symlink、inode、uid/gid 和 xattr 的 BTreeMap。

cgroup 控制文件是静态 seed，并不执行真正资源控制。cpuset `tasks` 写入被有意放行，以避免 BusyBox ash 在 LTP 中因预期写失败提前退出并留下后台进程；代价是部分“应失败”用例会 TFAIL。

xattr 名必须非空、<=255 且含点。cgroup 只接受非空 `trusted.*`，`security.*` 返回 Unsupported；普通 FS 交给后端。查询空 buffer 返回所需长度，缓冲不足由 FS/VFS 错误映射，syscall 层负责转换为 Linux errno。

## 13. 新增 VFS/文件 syscall 实例

例：新增一个按 fd 强制数据写回的 syscall：

```text
syscall handler
  -> 从 args 取 fd，校验 flags/用户参数
  -> fd-session 获取共享 OFD/handle guard
  -> handle.writeback() 或 handle.flush()
       ├─ writeback：仅 dirty page -> FS
       └─ flush：再做 backend sync
  -> VfsError 映射 ErrNo
  -> 释放 handle guard
```

实现前必须回答：

1. 是 fdatasync、fsync、syncfs 还是 close 语义；
2. directory fd 是否允许；
3. RO/pseudo/pipe/socket 返回什么；
4. 是否允许持 fd-table 锁进入 FS（通常不允许，应先 clone handle/OFD 引用）；
5. 写回失败是否原样返回，dirty 状态是否保留以供重试；
6. dup/fork 后共享 OFD 是否只写回一次且不会提前释放 open ref。

例：新增路径操作时的模板：

```text
syscall: copy user path + dirfd/cwd/root 解析 + symlink policy
  -> normalize absolute path
  -> assert_path_writable（若修改）
  -> resolve_route
  -> 检查同 mount/同 backend 约束
  -> 准备 cache/stable-node 事务状态
  -> 执行 FS 操作
  -> 仅成功后 commit cache/identity；失败 rollback
  -> VfsError -> 精确 errno
```

## 14. 常见故障定位

| 症状 | 优先检查 |
|---|---|
| unlink 后旧 fd 读错新同名文件 | stable node/detached commit/cache key |
| rename 后 writeback 报 NotFound | rename 前 flush 与 `finish_rename` 是否执行 |
| mount 后内存上涨/脏数据消失 | cache flush warning 后仍 bump generation |
| remount-ro 仍可写 | 每个修改入口是否走 `assert_path_writable` |
| remount-ro 后 getxattr 被拒绝 | 是否错误用了 `fs_and_rel_rw` 而非 query helper |
| fork 后 mount 意外互相可见 | CLONE_NEWNS 的 copy 与普通 clone 的 share 是否接反 |
| unshare owner 后 namespace 消失 | owner re-home/refcount/namespaces 三表一致性 |
| bind 路径 InvalidPath | bind 环或超过 32 层；组件边界匹配 |
| close 返回 EIO但数据已部分写 | page-cache flush 的逐页进度与后端短写 |
| Drop 只打印写回失败 | fd close 路径未显式调用 handle.close |
| getdents 看不到新文件 | DirectoryHandle 是首次读取 snapshot |
| proc 数值不刷新 | ProcFileHandle 内容在 open 时 snapshot |
| 大文件 open-unlink 返回 Io | 无 stable node fallback 的 16 MiB 上限 |
| O_APPEND 并发覆盖 | OFD reservation、current logical size、不同 OFD 原子性 |

排查写回时同时记录 `mount_gen/mount_id/node_id/cache_key/path/open_refs/logical_size/dirty pages/detached`，只打印 pathname 通常不足以区分旧 node 和新同名文件。

## 15. 修改检查清单

### 新 FS backend

- 实现 read/write range、metadata、sync 和明确错误；
- RW 普通文件优先实现 node open/close/read/write/truncate/link；
- mount identity 不与其它实例混淆；
- readonly 与 xattr query 分开；
- rename/unlink 与 open handle 测试；
- short write=0 必须被识别为 I/O 错误，避免死循环；
- 所有大分配使用可失败路径或显式上限。

### 新 mount 类型

- 加 `AuxMount/FsRoute` 和最长前缀解析；
- exists/metadata/read/read_range/read_dir/open 全部路由；
- write/xattr/sync 的 RO 规则；
- statfs magic、`/proc/mounts` device/fstype；
- namespace clone/unshare/move/bind/unmount；
- generation/cache 隔离；
- 不持 namespace registry 锁调用后端。

### 改 stable/cache identity

- mount generation、mount id、node id 三者语义；
- hardlink 是否共享 node identity；
- rename replacement 与 rollback；
- unlink 后最后 lease 的 `close_node`；
- mmap/exec 的 content version；
- open ref 在 clone/close/Drop/prepared-read 的精确配对。

## 16. 回归矩阵

静态门禁：

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

功能回归：

| 类别 | 必测场景 |
|---|---|
| path | 根、`.`/`..`、长路径、非法 NUL/UTF-8、symlink follow/no-follow |
| mount | 最长前缀、bind 环、recursive bind、move、RO、unmount |
| namespace | 普通 fork 共享、CLONE_NEWNS 隔离、owner/member unshare、exit 顺序 |
| file | read/write/pread/pwrite/seek/O_APPEND/O_SYNC/short buffer |
| lifetime | open-unlink-recreate、open-rename、replace target、hardlink |
| cache | 跨页、truncate grow/shrink、dirty eviction、fsync error retry |
| close | 显式 close 错误、dup 后单 fd close、最后 ref Drop |
| directory | 小 buffer EINVAL、rewinddir、snapshot 行为、fork 共享 cookie |
| pseudo | proc/sys reopen refresh、namespace magic fd、securityfs root |
| tmp/cgroup | size limit、xattr、cgroup v1/v2 seed、cpuset 脚本不挂死 |
| OOM | prepared-read staging、detached 16 MiB 边界、mount/dir 大列表 |
| long run | forkheavy + rename/unlink/fsync + mount namespace churn |

现有 `test()` 只覆盖 API smoke、mount identity/基本 route 和 prepared-read 分配失败；它不能证明上述并发、持久化和生命周期语义。运行通过时必须准确描述覆盖范围，不能把 self-test 当成完整 VFS 回归。

当前在 x86_64/macOS 直接运行 `cargo test -p wateros-vfs-impl-fs-bridge --lib` 会先在依赖链的 `sbi-rt` 因 RISC-V `a0..a7` 寄存器不可用而编译失败，尚未执行本 crate 的 `mount_ns` unit tests。这是 host-test 隔离缺口，不代表测试通过或失败；要让这些纯数据结构测试可离线运行，应把 task/platform 依赖隔离到 feature 或为 host 提供无硬件 mock。修复前以目标架构 `make check` 加 QEMU self-test/功能用例作为证据。
