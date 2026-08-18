# fs-rootfs

[返回 wateros-fs](../README.md) · [devfs](../fs-devfs/README.md) · [VFS](../../wateros-vfs/README.md)

rootfs 保存当前活动文件系统实现和已挂载根卷句柄。它负责“哪个 backend/哪个块设备是根”，不负责路径解析、per-task root/cwd、mount namespace 或页缓存；这些属于 VFS。

## 状态

[`registry.rs`](rootfs-impl/impl-kernel/src/registry.rs) 中四个独立 mutex 保存：

| 状态 | 含义 |
| --- | --- |
| `ACTIVE_FS_IMPL` | 聚合层 probe 后选择的 `'static dyn FsImpl` |
| `ROOT_FS` | 根卷只读共享句柄，供 ELF loader/内核读取 |
| `ROOT_RW_FS` | 根卷读写共享句柄，供 VFS 修改路径 |
| `ROOT_DEV_PATH` | 最近成功挂载使用的 devfs 路径 |

[`state.rs`](rootfs-impl/impl-kernel/src/state.rs) 的 `MOUNT_GENERATION` 在成功挂载或 mount view 变化后递增，VFS 可用它识别需要失效的缓存视图。

## 启动调用链

```text
fs::init_after_boot
  -> devfs default root
  -> registered FsImpl probe
  -> rootfs::set_active_fs_impl

user_bringup_bus
  -> mount_default_root_rw
  -> devfs::lookup_block_device
  -> active impl mount_rw(device)
  -> create SharedFs adapter over the same RW instance
  -> 提交 ROOT_FS + ROOT_RW_FS + ROOT_DEV_PATH
  -> bump_mount_generation
```

RO 与 RW handle 都必须建立，但它们必须共享同一个已挂载的实例。`ROOT_FS` 是
`ROOT_RW_FS` 的只读适配视图：这既满足 ELF loader 的只读接口，也避免同一块设备上两个
ext4 实例产生目录项与元数据缓存分叉。

当前 `mount_root_rw_from_block_path` 顺序创建 RO、RW 后逐项写全局槽。修改时要考虑第二次 mount 失败与重复 mount：理想事务是先把所有可失败对象构造在局部变量中，再一次性提交全局状态；不能先清旧根再发现新根挂载失败。

## 辅助挂载

`mount_aux_ro_from_block_path` 与 `mount_aux_rw_from_block_path` 创建独立卷句柄，不替换根槽。若 path alias 最终指向与根相同的 `Arc` block device：

- RO 可复用已有 `ROOT_FS`；
- RW 可复用已有 `ROOT_RW_FS`；
- 避免对同一设备建立互不知情的多份 RW backend/cache。

比较设备身份应使用 `Arc::ptr_eq` 或稳定 device identity，不能只比较路径字符串，因为 `/dev/vblk0`、`/dev/vda`、`/dev/vda1` 当前可能是同一对象。

## 修改与回归

新增根选择策略时定义：多盘优先级、无设备、probe miss、RO-only backend、重复挂载和 alias。新增 unmount/switch-root 必须先处理 VFS mount/page cache/open handles 与写回，再改变 generation 和全局槽。

最小回归：无盘应返回 `NotMounted` 而非 panic；坏 superblock probe miss；正常镜像同时可读 ELF 和写文件；同盘 alias auxiliary mount 不创建第二个 RW 实例；重复启动/挂载不泄漏 handle。
