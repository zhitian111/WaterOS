# wateros-fs-devfs-api-v0 离线开发手册

本 crate 定义驱动注册表到 `/dev` 设备树视图之间的版本化契约：节点快照、块/字符设备路径
绑定、DTB 未支持占位和默认根块设备选择。它不拥有设备、不执行硬件探测，也不是普通磁盘
文件系统。实现细节见 [devfs impl-kernel](../../devfs-impl/impl-kernel/README.md)，父级概览见
[fs-devfs](../../README.md)，设备接口见 [wateros-driver](../../../../wateros-driver/README.md)。

## 数据结构

`DevNode` 只包含逻辑绝对路径和粗类型：

| `DevNodeType` | 含义 | 是否保证可 I/O |
| --- | --- | --- |
| `Block` | 路径应绑定 `SharedBlockDevice` | 应可由 `lookup_block_device` 找到 |
| `Character` | 字符节点或 VFS 内建特殊节点 | 不保证绑定 driver；例如 `/dev/zero` 可由 VFS 实现 |
| `Unsupported` | DTB 枚举到但未绑定支持驱动 | 否，不得制造假 handle |

因此 `list_nodes()` 是目录/诊断快照，不是“每项都可 lookup”的保证。调用方必须根据 node type
和具体路径走正确 lookup 或 VFS 内建 handle。

`SharedBlockDevice`/`SharedCharacterDevice` 是共享设备对象。不同路径可以合法返回同一个 Arc，
例如 `/dev/vblk0`、`/dev/vda`、`/dev/vda1` 当前可能都是同一整盘对象。路径相等与设备 identity
是两个概念；挂载冲突判断必须比较共享对象/稳定设备 ID。

## `DevFsManager` 契约

| 方法 | 语义 | 失败/生命周期 |
| --- | --- | --- |
| `refresh` | 从当前 driver registry 重建发布快照 | 旧路径绑定可消失；调用者不应保存裸索引 |
| `set_dt_unsupported_paths` | 保存下次 refresh 合并的占位路径 | 不立即保证 `list_nodes` 可见 |
| `list_nodes` | 返回节点 Vec 副本 | 快照返回后可因 refresh 过期 |
| `register_*_device` | path 绑定共享设备 | 当前实现同路径替换；新实现需明确冲突语义 |
| `lookup_*_device` | 按精确路径 clone handle | 未绑定返回 `FsError::NotFound` |
| `default_root_block_path` | 返回根卷探测候选路径 | 无块设备返回 `None` |

API 使用 `FsResult/FsError` 方便 rootfs/VFS 统一映射，但这里的错误仍不是 syscall errno。用户
字符串、权限、mount flags、`stat` 与 open handle 都属于 VFS/syscall 层。

## 刷新与并发边界

推荐实现顺序：

```text
读取 block registry 快照
→ 读取 character registry 快照与 kind
→ 复制 DTB unsupported paths
→ 获取 devfs 状态锁
→ 在局部/锁内重建 nodes 与 bindings
→ 一次发布一致快照
→ 解锁后记录日志或通知 VFS
```

不能持 devfs 锁再调用可能取得 driver registry 锁的复杂路径，否则另一条“driver 锁 →
devfs refresh”链会形成 ABBA。`lookup_*` 应在锁内找到并 clone Arc，然后立刻释放锁；设备 I/O
必须在锁外进行。

## 新增设备种类实例

假设增加随机数字符设备：

1. 在 character driver API/registry 定义或复用明确的 `CharacterDeviceKind`。
2. 平台驱动注册真实 `SharedCharacterDevice`。
3. devfs 实现的 refresh 为该 kind 添加 `/dev/hwrng` alias，同时放入
   `character_bindings` 和 `nodes`。
4. VFS open 路径先尝试 `lookup_character_device`，构造设备 handle；读写、poll、ioctl 由字符
   设备接口转发。
5. syscall 仍经过普通 open/read/ioctl 的用户复制与 errno 层，不能直接查 devfs 全局状态。
6. 测试无设备、一个设备、多设备、重复 refresh、alias 同 Arc 和并发 open。

只向 `nodes` 添加字符串会让 `ls /dev` 可见但 open lookup 失败；只添加 binding 则 lookup
可能成功但目录枚举不可见。真实 driver 节点通常要同时更新两者。

## 默认根设备策略

`default_root_block_path` 是策略接口，不表示已 mount 或已识别文件系统。返回路径后还有：

```text
lookup_block_device
→ 所有 FsImpl probe
→ 选择能力匹配后端
→ rootfs mount_ro/mount_rw
```

多盘环境必须定义稳定优先级。仅使用“注册表第一个”会受驱动枚举顺序影响；比赛若传入明确
root 参数，应由上层策略优先使用该路径，并保留不存在/probe miss 的诊断。

## 修改检查清单

- [ ] 节点路径是规范绝对路径，重复 alias 处理明确。
- [ ] 真正 I/O 节点同时拥有 nodes 项与正确 binding。
- [ ] unsupported/内建特殊节点没有伪造 driver handle。
- [ ] refresh 不在 devfs 锁内反向执行 driver I/O/复杂注册表调用。
- [ ] lookup clone handle 后释放锁，设备操作在锁外。
- [ ] 设备 identity 与路径 alias 没有混用。
- [ ] 默认根选择覆盖无盘、多盘和显式路径。
- [ ] 新错误按 `FsError → VfsError → ErrNo` 链映射。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

运行期核对 refresh 前后节点数、`/dev` 枚举、每个真实节点的 open/read/write/ioctl，以及默认
根路径能 lookup 并通过正确后端 probe。分区别名目前不是独立分区对象，测试不能假设其 LBA
已偏移。

