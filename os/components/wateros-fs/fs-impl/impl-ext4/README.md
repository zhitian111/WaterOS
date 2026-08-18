# impl-ext4（ext4plus）离线开发手册

本 crate 是基于 `ext4plus 0.1.0-beta.3` 的可选 ext4 RO/RW 后端。它不是当前默认后端；
`wateros-fs` 默认选择 [impl-another-ext4](../impl-another-ext4/README.md)，且三个 ext4 feature
互斥。通用契约见 [fs-api](../../fs-api/api-v0/README.md)，启动/选择链见
[wateros-fs](../../README.md)。

## 启用方式与定位

聚合层 feature 名为 `impl-ext4`。启用后注册 `IMPL: Ext4FsImpl`，声明 Ext4 RO/RW 能力；probe
仅读取 superblock `1024 + 0x38` 处的 `0xEF53` magic。magic 匹配只代表 ext2/3/4 家族候选，
不证明 feature、checksum、journal 或目录结构都受库支持，完整错误会在 mount 时出现。

不要同时启用 `impl-ext4`、`impl-ext4-rs`、`impl-another-ext4`；聚合 crate 有 compile_error。
切换后端时必须重新跑完整镜像回归，不能以 `cargo check` 代替磁盘一致性验证。

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | probe、`FsImpl`、RO/RW 句柄包装 |
| `src/ro.rs` | `Ext4Read` 适配、RO 路径/metadata/readdir/range read |
| `src/rw.rs` | `Ext4Write` 适配、路径写、RMW、small-read cache、属性/xattr |
| `src/boot_inspect.rs` | 启动 DFS 中 shell/ELF 头诊断 |
| `src/selftest.rs` | 根 metadata、range-read、mkdir 烟测 |

`mount_ro` 和 `mount_rw` 会在同一 block device 上构造两个独立 `ext4plus::Ext4` 实例，并分别
包装为 `SharedFs`/`SharedRwFs`。rootfs 当前需要两个视图，但它们可能各自缓存 metadata；RW
修改后 RO 可见性必须实测，不能假设库自动保持双实例一致。

## RO 路径

`Ext4Fs { fs: Option<Ext4> }` 在 mount 前所有操作返回 NotMounted。已实现：exists、metadata、
read_dir、整文件 read、read_range、read_prefix 和 boot tree dump。

读实现按 WaterOS `BLOCK_SIZE` 分片调用 `File::read_bytes`，源注释说明这是为规避 VirtIO 512B
扇区组合下大块整读出现 ELF 头损坏。EOF 为短读/0；整文件 read 若未填满 metadata size 则
返回 Io。目录跳过 `.`/`..`，非 UTF-8 文件名返回 NotUtf8。

RO trait 没有覆盖 `read_symlink`，因此当前默认返回 Unsupported；metadata 使用
`FollowSymlinks::All`，观察到的是目标而非链接本身。这意味着需要精确 lstat/readlink 的 VFS
路径不能把本后端当完整实现。

启动 DFS 仅用于诊断：只对名字以 `_testcode.sh` 结尾的脚本最多读取 4096 字节并显示 512
字符，对带执行位的 ELF64 little-endian 打印 machine/entry。不要把这个 parser 当 ELF loader。

## RW 路径与块适配

`Ext4FsRw` 通过 `BlockDevRw` 实现 ext4plus 的读写接口。非块对齐写使用：

```text
不对齐头：读整块 → patch → 写整块
中间完整块：直接 write_blocks
不对齐尾：读整块 → patch → 写整块
```

全局 `EXT4_SMALL_READ_CACHE` 只缓存一个 `(Arc device identity, block)`，仅服务不超过 64 字节且
不跨块的小读；写路径必须按覆盖区间使缓存失效。缓存锁不能与 block-device 锁反序持有，
也不能把 Arc 地址 identity 当跨销毁重建永久 ID。

源码明确标注 RW 路径为 beta，且没有完整 journal/崩溃一致性保证。已覆盖的路径级能力包括：

- 创建/替换普通文件、range write、truncate、mkdir；
- chmod/chown、set/get/list/remove xattr；
- unlink/rmdir、rename、hardlink；
- exists/metadata/read/read_range/read_dir。

部分 rename/link 对目录、跨父目录或覆盖类型组合返回 Unsupported。未覆盖的 fs-api 方法继续
使用默认 Unsupported，尤其包括：

- `sync`；
- `open_node/close_node` 与全部 `*_node` 稳定 inode I/O；
- `create_tmpfile_node/link_node`；
- `symlink`、`mknod`；
- RW `read_symlink`。

所以本后端不能满足完整 open-unlink、`O_TMPFILE`、页缓存按稳定 inode 写回、fsync 持久化或
符号链接 syscall 语义。VFS bridge 若退回 path I/O，rename/unlink 后已打开 fd 的行为必须特别
验证；不要仅因 capability 表声明 RW 就认为所有 `ReadWriteFs` 方法已实现。

## 错误映射

ext4plus 错误映射到 `FsError`；未知/不兼容/readonly 等多种情况会折叠为 Unsupported。排查时
要在映射前保留原始错误日志，否则上层只能看到宽泛 errno。driver 错误经 boxed error 进入库，
block I/O 失败应最终为 Driver/Io，不能静默继续写 metadata。

probe 持 block device mutex 读取 2 字节；mount/operation 通过库回调多次锁设备。不要在外层
持同一设备锁调用本后端，否则会自锁。

## 调用链

```text
fs::init_after_boot
→ IMPL.probe(device)
→ rootfs::mount_default_root_rw
→ IMPL.mount_ro(device.clone) → Ext4::load(BlockDeviceReader)
→ IMPL.mount_rw(device)       → Ext4::load(BlockDevRw)
→ VFS fs-bridge
→ path ReadWriteFs method
→ ext4plus inode/dir/file operation
→ BlockDevRw read/write → WaterOS BlockDevice
```

当前没有后端 `sync`，所以 `fsync/syncfs` 若最终要求该方法会得到 Unsupported；不能把写 syscall
返回成功当成掉电持久化成功。

## 补稳定节点能力的路线

1. 确认 ext4plus 能以 inode identity 打开并在 unlink 后持引用。
2. 实现 `open_node` 返回 `FsNodeId(inode)`，维护每 inode open refcount。
3. 所有 node read/write/truncate/metadata 绕过路径重查。
4. unlink 时 nlink 归零但 open refcount 非零，延迟 inode 回收；最后 close 才 reclaim。
5. VFS page-cache key 同时带 mount generation 和 node ID。
6. rename 覆盖、open-unlink-read/write、fork/dup/close、mmap、失败回滚都加测试。
7. 实现 sync/flush 链并验证重启读回与 fsck。

没有库级 inode 生命周期支持时，不要伪造 node ID 后仍按路径操作；这会在路径复用时写错文件。

## 故障定位

| 现象 | 首查 |
| --- | --- |
| probe 成功、mount 失败 | magic 只是轻量识别；检查 ext4 feature/checksum/journal 兼容性 |
| RW 后 RO 仍读旧数据 | 两个独立 Ext4 实例缓存是否一致 |
| 长读 ELF 头损坏 | 是否绕过分块 read_range；block size/扇区适配 |
| 小 metadata 读偶发旧值 | small-read cache 是否在所有重叠写后失效 |
| rename/unlink 后 fd 错乱 | 没有稳定 node API，路径 fallback 生命周期不足 |
| readlink/symlink/O_TMPFILE 失败 | 对应 trait 方法当前 Unsupported |
| fsync 成功预期不成立 | 后端未实现 sync/journal 保证 |
| 只看到 Unsupported 无根因 | ext4plus 错误在映射时被折叠，增加原始错误诊断 |

## 回归

```bash
cd os
make check ARCH=rv PROFILE=pre EXTRA_FEATURES=
make check ARCH=la PROFILE=pre EXTRA_FEATURES=
```

真正启用该可选后端时，应在 feature 图中关闭默认 another-ext4 后单独构建。运行测试必须使用
镜像副本，覆盖跨块非对齐读写、sparse/truncate、xattr、link/rename、目录删除、双 RO/RW
实例可见性和错误注入；退出 QEMU 后执行 `e2fsck -fn`，重新启动读回数据。

