# fs-devfs

[返回 wateros-fs](../README.md) · [驱动组件](../../wateros-driver/README.md) · [VFS 组件](../../wateros-vfs/README.md)

本模块把 driver 子系统注册表投影成稳定的 `/dev` 节点和设备句柄查找表。它不探测硬件、不拥有设备对象，也不实现普通磁盘文件系统；设备的最终所有权仍在 driver registry，devfs 保存共享句柄和路径别名。

## 分层

| 目录 | 职责 |
| --- | --- |
| `devfs-api/api-v0` | `DevNodeType`、`DevNode`、`DevFsManager` 契约 |
| `devfs-impl/impl-kernel` | `DEVFS` 全局快照、设备路径绑定、refresh 和 `FsImpl` 能力声明 |
| `src/lib.rs` | feature 选择与稳定再导出 |

核心状态在 [`manager.rs`](devfs-impl/impl-kernel/src/manager.rs) 的 `DEVFS: Mutex<DevFsImpl>`：

- `nodes`：最近一次 refresh 的节点快照；
- `block_bindings`：路径到 `SharedBlockDevice`；
- `character_bindings`：路径到 `SharedCharacterDevice`；
- `dt_unsupported_paths`：DTB 已发现但无驱动的占位节点。

## refresh 链路

```text
driver 注册完成
  -> 先复制 block/character registry 快照
  -> 复制 unsupported DTB paths
  -> 获取 DEVFS 锁并清空旧 nodes/bindings
  -> 生成块设备与字符设备别名
  -> 加入特殊字符占位节点
  -> 加入 unsupported 节点
  -> 发布完整新快照
```

先取 driver 快照、后取 devfs 锁是刻意的锁边界；不要在持 `DEVFS` 锁时反向访问会获取 driver registry 的复杂路径。

块设备别名：每个设备有 `/dev/vblkN` 与 `/dev/vd<letter>`；第 0 块盘额外发布 `/dev/vda1`、`/dev/vda2` 的同设备别名。当前没有分区表解析，这些后缀只是兼容路径，不能据此认为已创建独立分区对象。超过 26 个设备时字母截到 `z`，因此扩容前必须重新设计唯一命名。

字符设备别名：`/dev/ttySN`；第 0 个 serial 映射 `/dev/console` 和 `/dev/tty`；RTC、null 设备按 kind 增加 Linux 风格别名。`/dev/zero`、`/dev/urandom`、`/dev/cpu_dma_latency` 可作为 VFS 内建特殊 handle 的节点存在，不一定绑定真实 character driver。

默认根设备优先 `/dev/vda`，否则取第一个 block node。修改枚举顺序会改变 fallback 根盘，应在多盘场景显式指定/验证。

## 新增设备节点

1. 先在 driver 子系统建立真实共享设备对象。
2. 确定是 registry 自动别名还是 VFS 内建特殊节点。
3. 在 `refresh` 中生成稳定、无重复的绝对路径。
4. 若需要真实 I/O，把 path 与共享 handle 同时加入 binding。
5. VFS open 路径识别 node type 并构造正确 `VfsIoHandle`。
6. 测试 refresh 两次、设备缺失、多设备和 alias 指向同一 Arc。

不要只往 `nodes` 加字符串：`lookup_*_device` 仍会 `NotFound`。也不要为 unsupported DTB 节点返回假 handle。

## 回归

```sh
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

启动后核对 driver registered count、devfs total/block/character/unsupported 数量、默认根路径，以及 `/dev` 的 stat/open/read/write 行为。多盘测试应确认 root 选择和 alias 不冲突。

refresh或lookup失败时不得发布指向不存在registry index的节点；设备缺失应返回稳定NotFound/Unsupported，不可用假handle掩盖驱动错误。
