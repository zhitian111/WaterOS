# wateros-fs-api-v0 离线开发手册

本 crate 是块文件系统实现、rootfs 管理和 VFS bridge 之间的版本化契约。它定义统一错误、
文件系统/节点类型、只读与读写 trait、稳定节点身份、实现注册接口和共享句柄包装；不负责
路径解析、mount namespace、fd offset、页缓存、用户指针或 errno。整体启动与后端关系见
[wateros-fs](../../README.md)，VFS 接线见
[impl-fs-bridge](../../../wateros-vfs/vfs-impl/impl-fs-bridge/README.md)。

## 源码地图

| 文件 | 内容 | 修改风险 |
| --- | --- | --- |
| `src/types.rs` | `FsError`、kind/access/capability、metadata、`FsNodeId` | 错误与类型会被所有后端和 VFS match |
| `src/traits.rs` | `ReadOnlyFs`、`ReadWriteFs`、`FsAsyncIo` | 默认方法多为 `Unsupported`，新增必需方法会破坏全部实现 |
| `src/handles.rs` | trait object 包装、共享句柄、`FsImpl` | unsafe `Send` 与 mutex 边界、注册和挂载入口 |
| `src/lib.rs` | 统一重导出 | 公共契约必须从这里可见 |

依赖方向：

```text
block device API
      ↓
fs-api-v0
  ├─→ ext4/ramfs/devfs/procfs 实现
  ├─→ rootfs manager
  └─→ VFS fs-bridge → syscall → 用户态
```

API 不应依赖 VFS 或 syscall，否则会形成反向依赖；权限、cwd、dirfd、mount 路由与用户复制
都应在上层完成后，才把规范化的后端路径和内核切片交给本接口。

## 基础类型

### `FsError` 的传播边界

`FsError` 表示后端语义，不是 Linux errno。目前 VFS bridge 对每个枚举有显式
`FsError → VfsError` 映射，syscall 再转成 `ErrNo`。新增错误变体至少要同步：

1. 所有具体 FS 后端的返回点；
2. `wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs::map_fs_error`；
3. VFS 到 syscall 的 errno 映射；
4. 用户态负例测试。

常用语义：

| 错误 | 后端含义 | 典型上层 errno 意图 |
| --- | --- | --- |
| `NotMounted` | 根卷/句柄未就绪 | `ENODEV` 或调用点约定 |
| `NotFound` | 路径/节点不存在 | `ENOENT` |
| `NotAFile` | 类型不满足操作 | 需由上层区分 `EISDIR/ENOTDIR` |
| `InvalidPath` | 后端路径非法 | `EINVAL/ENAMETOOLONG` 由上层细化 |
| `Exists` | 目标冲突 | `EEXIST` |
| `NotEmpty` | 删除非空目录 | `ENOTEMPTY` |
| `Unsupported` | 后端没有能力 | `ENOSYS/EOPNOTSUPP` 由调用点决定 |
| `Driver/Io/Corrupt` | 块层、通用 I/O、磁盘格式错误 | 通常 `EIO` |
| `NoSpace` | 空间/资源耗尽 | `ENOSPC` |

不要在后端用 `Unsupported` 掩盖“路径不存在”或“权限拒绝”；上层只能依据枚举做 errno 映射，
错误分类越粗，Linux 兼容测试越难修。

### kind、能力和 I/O 模式

- `FsKind` 描述 ext2/3/4、DevFs、RamFs 或具名 `Other`。
- `FsAccessMode` 只有 RO/RW；它描述挂载能力，不是单次 open flags。
- `FsCapability` 是 `FsImpl::supported()` 静态表的一项。
- `FsIoMode::{Direct,Async}` 是配置/策略标识；`Async` 当前未实现。
- `FsAsyncIo` 是占位 trait，默认方法都返回 `Unsupported`，不能据此宣称异步 I/O 可用。

`FsImpl::supports` 只是遍历静态能力表。probe 返回某 kind 后，聚合层仍会检查该 kind 与所需
访问模式是否在表中；能力表、probe 与实际 `mount_*` 必须三者一致。

### 元数据与稳定身份

`FsMetadata` 是路径或节点查询时的快照：类型、size、mode、inode、nlink、uid、gid。它不是
完整 `stat` ABI，时间戳、blocks、rdev 等可能由 VFS/具体节点补充。硬链接路径必须报告相同
inode，目录 size 可以是实现定义值。

`FsNodeId(u64)` 是“单个已挂载 FS 实例内”的稳定节点身份：

- 只能由后端构造，调用者只可把 `raw()` 用作缓存键/诊断；
- 必须与 mount generation/mount id 一起使用，不能跨卸载复用；
- `open_node` 成功就取得一次后端 open 引用，最终必须恰好一次 `close_node`；
- unlink/rename 后，已打开 node 仍应指向原 inode，而不是重新按旧路径查找；
- unnamed tmpfile 在发布前由 open 引用维持，最后关闭时才可回收。

这是 `open-unlink-read`、mmap、页缓存、`O_TMPFILE + linkat` 正确性的核心，不可用路径字符串
替代。

## 只读接口

`ReadOnlyFs` 的必需方法是 mount 状态、exists、metadata 和整文件 read。其余有默认值：

| 方法 | 默认行为 | 实现建议 |
| --- | --- | --- |
| `read_range` | `Unsupported` | 大文件、ELF、mmap 后端必须高效实现 |
| `read_prefix` | 调用整文件 `read` 后 truncate | 后端可覆盖，避免大文件全量分配 |
| `read_to_string` | 整文件 read + UTF-8 校验 | 配置小文件适用 |
| `read_dir` / `read_symlink` | `Unsupported` | VFS 目录与 symlink 支持所需 |
| `boot_dump_all_paths` | no-op | 仅诊断，不能成为启动正确性依赖 |

路径通常要求绝对路径，但 API 没有统一验证器。VFS bridge 应先规范化；实现也应防御 `..`、
空组件、越界长度和 NUL 等非法输入。EOF 必须表现为短读或 0，不能把短读当 I/O 错误。

## 读写接口

`ReadWriteFs: Send` 与 RO trait 独立。除 `mount_rw`、`is_mounted`、
`write_regular_file_at_root` 外，多数方法默认 `Unsupported`。功能分组如下：

| 分组 | 方法 | 核心不变量 |
| --- | --- | --- |
| 持久化 | `sync` | VFS 先写回文件页缓存，后端再提交 FS/块缓存 |
| 稳定节点 | `open/close/metadata/read/write/truncate_node` | identity 不随 rename/unlink 改指向；open 引用配对 |
| unnamed inode | `create_tmpfile_node`、`link_node` | 创建即持 open 引用；发布与最终回收明确 |
| 路径写 | write、unlink/rmdir、mkdir、truncate、rename/link/symlink/mknod | 绝对后端路径、类型/原子性/同挂载约束 |
| 属性 | chmod/chown、xattr | mode/uid/gid 与长度探测语义一致 |
| 路径读 | exists/metadata/read/read_range/read_dir/read_symlink | RW handle 可作为读视图，但仍受 mutex 串行化 |

`getxattr/listxattr` 约定空缓冲用于查询所需长度；`listxattr` 的返回长度包含 NUL 分隔/结尾。
`chown` 的 `None` 表示对应字段不改。`mkdir` 接收的 mode 尚未应用 umask，umask 应在 syscall
或 VFS 权限层处理。

`sync` 不等价于单个 fd 的 `fsync`：VFS 的 paged handle 可能仍有脏页。正确链路通常是：

```text
sys_fsync/fsync-range/syncfs
→ VFS 找到稳定节点并写回 page cache
→ ReadWriteFs::write_range_node / truncate_node
→ ReadWriteFs::sync
→ 文件系统缓存提交
→ 块缓存 flush
→ BlockDevice flush（若设备支持）
```

若跳过前半段，后端 `sync()` 成功也可能仍有数据只存在 VFS 页缓存。

## trait object 与共享句柄

```text
SharedFs   = Arc<Mutex<LocalFs(Box<dyn ReadOnlyFs>)>>
SharedRwFs = Arc<Mutex<LocalRwFs(Box<dyn ReadWriteFs>)>>
```

`LocalFs`/`LocalRwFs` 逐项转发 trait，并用 `unsafe impl Send` 允许装入跨线程 `Arc`。实际串行化
依赖外层 `spin::Mutex`；这项 unsafe 承诺要求任何具体实现都不能在无保护的内部可变状态上
产生数据竞争。新增 trait 方法时必须同步在两个 wrapper 中转发，否则调用可能落到默认
`Unsupported` 或直接无法编译。

取得 `Shared*` 锁后不要做用户复制、阻塞等待或获取会反向进入 FS/VFS 的锁。尤其不能在持有
后端 mutex 时等待页缓存回写线程，而回写线程又需要同一个句柄。

## `FsImpl` 注册与挂载

每个后端暴露一个 `'static dyn FsImpl`，聚合层建立静态注册表。启动探测链：

```text
fs::init_after_boot
→ devfs refresh + default_root_block_path
→ 对 registered_fs_impls 依序调用 probe(device)
→ probe 返回 Some(kind) 且 supports(kind, RW/RO)
→ rootfs::set_active_fs_impl(imp)
→ bring-up 稍后 mount_default_root_rw
→ imp.mount_ro(device.clone) + imp.mount_rw(device)
```

`probe` 应只读取必要 superblock，不修改设备，不长期持锁，不把普通“不是我的格式”返回为
`Corrupt`；不识别应 `Ok(None)`。`mount_ro` 是必需入口，`mount_rw` 默认 Unsupported。

### 新增后端的最小实例

```rust
static CAPS: &[FsCapability] = &[
    FsCapability::new(FsKind::Other("myfs"), FsAccessMode::ReadOnly),
];

pub struct MyFsImpl;

impl FsImpl for MyFsImpl {
    fn name(&self) -> &'static str { "myfs" }
    fn supported(&self) -> &'static [FsCapability] { CAPS }
    fn probe(&self, dev: &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        // 读取固定 magic；不匹配返回 Ok(None)，I/O 失败返回 Driver/Io。
        todo!()
    }
    fn mount_ro(&self, dev: SharedBlockDevice) -> FsResult<SharedFs> {
        // 构造、mount，再包装为 Arc<Mutex<LocalFs>>。
        todo!()
    }
}

pub static IMPL: MyFsImpl = MyFsImpl;
```

随后在 `wateros-fs/Cargo.toml` 添加 feature/依赖，在 `registered_fs_impls()` 注册，并覆盖
probe miss、坏 magic、短读、重复挂载和实际文件读测试。

## 为 syscall 补文件操作的调用模板

以新增后端能力对应的 syscall 为例，层次应保持：

```text
syscall：解析寄存器、copy 用户字符串/结构、权限与 flags、返回 ErrNo
→ VFS：dirfd/cwd/namespace 路由、路径规范化、mount 选择、fd/offset/page cache
→ fs-bridge：VfsError ↔ FsError、稳定 node 和 mount generation
→ ReadWriteFs：只处理本挂载内绝对后端路径或 FsNodeId
→ 块设备
```

不要从 syscall 直接锁 `SharedRwFs`：那会绕过 mount namespace、符号链接策略、权限、页缓存
与稳定节点语义。若后端 trait 缺能力，先定义精确语义和默认 Unsupported，再同步 wrapper、
后端、bridge、syscall 与测试。

## 常见故障

- ELF/mmap 大文件导致堆暴涨：后端未实现 `read_range`，上层退回整文件 `read`。
- unlink 后 fd 读错新文件：实现按路径重新 lookup，没有使用 `FsNodeId`。
- `O_TMPFILE` 泄漏 inode：create 成功后没有在所有错误/关闭路径配对 `close_node`。
- `fsync` 后重启丢数据：只调用后端 sync，没先 flush VFS 脏页或块设备。
- 新错误导致 match 编译失败/errno 错：没有同步 bridge 与 syscall 映射。
- 后端宣称 RW 但 mount 失败：`supported` 表、probe 和 `mount_rw` 不一致。
- SMP 下偶发损坏：具体实现不满足 wrapper 的 `Send` 承诺，或绕过了 Shared mutex。

## 修改检查清单

- [ ] 新 trait 方法有默认兼容策略或已同步所有后端。
- [ ] `LocalFs/LocalRwFs` wrapper 已完整转发新方法。
- [ ] `FsError` 新变体已贯穿 VFS/errno 映射。
- [ ] 稳定节点 open/close 在成功、失败、fork、exec、close、unlink 路径都配对。
- [ ] 路径操作不绕过 VFS 的 namespace、权限、symlink 和页缓存层。
- [ ] probe 无副作用，能力表与 mount 能力一致。
- [ ] sync 测试覆盖页缓存、FS 缓存和块设备刷新顺序。
- [ ] 两个架构顶层 feature 组合均通过。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

涉及磁盘写入时使用镜像副本或 QEMU snapshot；持久化验证还应退出 guest 后对副本执行宿主侧
只读 `e2fsck -fn`，并重新启动检查数据，而不能只相信一次 syscall 返回 0。

