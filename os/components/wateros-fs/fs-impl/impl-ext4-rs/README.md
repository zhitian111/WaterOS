# impl-ext4-rs 离线开发手册

本 crate 是基于 `ext4_rs 1.3.3` 的可选 ext4 RO/RW 适配。它不是默认后端；默认是
[impl-another-ext4](../impl-another-ext4/README.md)。它实现了自己的路径/symlink 解析和若干
namespace 写操作，但底层库 API 与错误传播仍有明显限制。通用契约见
[fs-api](../../fs-api/api-v0/README.md)，聚合选择见 [wateros-fs](../../README.md)。

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `src/core.rs` | block adapter、错误/metadata 映射、路径/symlink lookup、FS 状态 |
| `src/operations.rs` | RO/RW trait、probe、`FsImpl` 注册 |
| `src/lib.rs` | 重导出 core |

feature 名是 `impl-ext4-rs`，与另外两个 ext4 后端互斥。`IMPL` 声明 Ext4 RO/RW，probe 同样只
检查 `0xEF53` magic。

## 核心状态与挂载

`Ext4RsFs { fs: Option<ext4_rs::Ext4> }` 同时实现 RO/RW。`mount_rw` 直接调用 RO `mount`，两者
没有只读保护差异。`FsImpl::mount_ro` 与 `mount_rw` 各自构造一个实例，所以 rootfs 主路径仍
会对同一 device 建立两份 Ext4 对象；缓存/写入可见性需要实测。

块设备包装为 `Arc<dyn ext4_rs::BlockDevice>`。ext4_rs 接口的 `read_offset` 返回 Vec、
`write_offset` 返回 `()`，无法向库上传播 WaterOS driver error。当前适配代码是：

```text
read_offset: 分配 4096 字节 → read_bytes → 忽略 Result → 返回缓冲
write_offset: block_write_bytes → 忽略 Result
```

这意味着 I/O 失败可能变成全零/部分旧数据继续解析，写失败也可能被上层误报成功。这是本后端
目前最严重的可靠性限制；在修复库接口或建立 sticky error side-channel 前，不应把它用于要求
持久化正确性的 final 配置。

## 路径和 symlink

路径从 root inode 2 逐组件 `fuse_lookup`。规则：

- 中间 symlink 总是跟随，末级由 `follow_final` 控制；
- 最多 40 次 symlink 展开，超限映射 InvalidPath；
- 相对 target 基于链接所在已解析目录，绝对 target 从根开始；
- `.`/`..` 在展开 target 时规范化，直接输入 lookup 中的 `.`/`..` 被拒绝；
- 单组件最长 255；
- fast symlink（≤60 字节、0 blocks）直接从 inode `i_block` 小端 words 取文本。

RO `metadata` 不跟随末级 symlink，`read` 跟随，`read_symlink` 读取链接本体；这比 ext4plus
可选后端更接近 lstat/readlink 需求。VFS 仍需自行决定 syscall 是 follow 还是 nofollow。

## 已实现能力

RO：exists、metadata、整文件/range/prefix read、readdir、read_symlink、boot DFS。

RW 路径源码实现：

- 创建/替换普通文件、write_range、跨 EOF 零填充、truncate；
- unlink/rmdir、mkdir、chmod/chown；
- hardlink、rename（部分目录/覆盖组合 Unsupported）；
- symlink、mknod；
- 复用所有 RO 查询。

未覆盖的 fs-api 方法使用默认 Unsupported：

- `sync`；
- `open_node/close_node` 与稳定 node I/O；
- `create_tmpfile_node/link_node`；
- 所有 xattr。

因此同样不能保证 open-unlink 生命周期、`O_TMPFILE`、稳定页缓存写回、xattr syscall 或真实
fsync。rename 中部分目录情形明确 Unsupported，不能宣称完整 POSIX rename。

## 写入与 sparse 语义

byte-level block 写对不对齐头尾执行 read-modify-write，中间整块直接写。`write_range` 在
offset 超过 EOF 时显式 `zero_extend_file(old_size, offset)` 再写，避免库自动分配未清零洞。
truncate 增长也必须保证新区域读零，缩短需释放/更新 extent 与 inode size。

由于 block adapter 吞错误，即使这些算法返回 Ok，也不证明物理写成功。任何可靠修复至少应：

1. 给 adapter 增加 `AtomicBool/Mutex<Option<DriverError>>` sticky error；
2. 每次库操作前后检查并清晰决定错误归属；
3. 写失败后禁止继续提交成功 metadata；
4. 提供 flush 并把错误传到 `ReadWriteFs::sync`；
5. 错误注入验证短读、读失败、头/尾 RMW 写失败和 flush 失败。

更理想的是上游 ext4_rs 的 BlockDevice trait 原生返回 Result，避免 side-channel 无法精确关联
操作。

## 错误映射

`map_ext4_rs` 将 ENOENT/EEXIST/ENOTEMPTY/类型/路径错误映射到公共 FsError；ENOSPC 当前与 EIO
一起映射为 `FsError::Io`，会丢失 NoSpace 语义；其它大量 errno 也折叠为 Io。修复时同步 VFS
和 syscall errno 回归，不要只调整一个 match。

`Ext4::open(dev)` 当前不返回 Result，mount 无法直接报告 superblock load 失败；配合被吞的 read
error，坏设备可能推迟到后续操作才表现异常。

## 调用链

```text
fs::init_after_boot → Ext4RsImpl::probe
→ rootfs mount RO/RW
→ Ext4::open(BlockDevAdapter)
→ VFS path operation
→ lookup_inode[_follow]
→ ext4_rs fuse/read/write/create API
→ BlockDevAdapter read_offset/write_offset
→ WaterOS BlockDevice
```

外层 SharedFs/RwFs mutex 串行一个实例，但 RO/RW 是两个不同实例；block device mutex 只保护单次
I/O，不提供跨多个 metadata/data 写的事务。

## 补全为可用后端的优先顺序

1. **错误传播**：先解决 adapter 吞 I/O error 和 mount 不报错。
2. **sync/flush**：定义 library metadata flush、block flush 和 sticky error 的顺序。
3. **稳定节点**：inode identity、open refcount、unlink orphan、最后 close reclaim。
4. **rename/link 原子性**：目录、覆盖、失败回滚、跨父目录。
5. **xattr/tmpfile**：按 fs-api 契约补齐，不能用路径伪装 stable node。
6. **崩溃一致性**：journal/recovery 能力确认与断电/错误注入。
7. **双实例一致性**：rootfs 的 RO/RW handle 是否能安全共存。

## 故障定位

| 现象 | 首查 |
| --- | --- |
| 坏盘仍 mount 成功、后续出现 Corrupt | Ext4::open 无 Result，adapter read error 被忽略 |
| write 返回成功但重启丢失 | write error/flush 无法传播，且 sync 未实现 |
| ENOSPC 显示 EIO | map_ext4_rs 把 ENOSPC 映射 Io |
| symlink 循环报 InvalidPath | 40 跳上限的当前映射 |
| rename 目录返回 Unsupported | 源码只实现部分组合 |
| unlink 后已开 fd 错文件 | 没有 stable node/open-ref 生命周期 |
| xattr/O_TMPFILE/fsync 失败 | 对应 trait 当前默认 Unsupported |
| RW 后 RO 视图旧数据 | 同设备两份 Ext4 实例缓存 |

## 回归

除双架构构建外，必须在镜像副本执行：symlink 相对/绝对/fast/循环、跨块读写、sparse 零洞、
truncate 增缩、mkdir/rmdir、hardlink/rename 覆盖、mknod，以及设备错误注入。每个写测试都要
退出 guest、宿主 `e2fsck -fn`、重新启动读回；在错误传播问题修复前，单次 syscall 返回 0
不是通过证据。

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
