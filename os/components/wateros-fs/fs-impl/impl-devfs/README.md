# 简化 devfs 兼容实现手册

本 crate 是早期/测试用途的简化块设备视图，不是当前内核 `/dev` 的主实现。生产路径使用
[fs-devfs](../../fs-devfs/README.md) 与
[impl-kernel](../../fs-devfs/devfs-impl/impl-kernel/README.md)。文件系统稳定错误类型见
[fs-api](../../fs-api/api-v0/README.md)。

## 1. 为什么仍然存在

`wateros-fs` 的 Cargo workspace 和依赖仍包含此 crate，`self_test` feature 也会调用它的
`self_test()`；因此不能把它当死目录忽略。但普通内核启动的 `wateros_fs::devfs` 重导出的是
独立 `fs-devfs` crate，`init_after_boot()` 调用 `devfs::active_impl::refresh()`，不会使用这里的
`refresh()`。

此实现只枚举 block registry，适合 host/self-test 和兼容旧调用者。它没有：

- character device、`/dev/null`、RTC 或 console；
- DTB unsupported 占位；
- path 到 handle 的持久 binding 表；
- `FsImpl` mount/probe 能力；
- 动态注册、移除通知、目录 generation 或 VFS inode；
- GPT/MBR 分区解析。

新增生产功能时优先修改 `fs-devfs/devfs-impl/impl-kernel`，除非明确还要保持这个兼容视图。

## 2. 数据结构

```text
DEV_NODES: Mutex<Vec<DevNode>>

DevNode:
  path: String
  node_type: DevNodeType::{Block, Unsupported}
  index: usize             // driver block registry index
```

当前 `refresh` 只生成 `Block`，`Unsupported` 是未使用预留。`DEV_NODES` 只缓存目录描述，不
保存 `SharedBlockDevice`；设备对象仍由 driver registry 所有。

`list_nodes()` 会 clone 整个 Vec 和每个 String，适合少量 bring-up 节点，不适合高频 proc/debug
采样。mutex 保护 clear/rebuild 与 clone，所以读者不会看到半张表，但 refresh 在锁内格式化并
分配字符串。

## 3. 命名规则及其陷阱

对每个 driver index：

- 总是生成 `/dev/vblk{index}`；
- 生成 `/dev/vd<letter>`，0→a，25→z；
- index 0 额外生成 `/dev/vda1`、`/dev/vda2`。

`linux_vd_disk_path` 把 index 截到 25，所以 index >= 26 都得到 `/dev/vdz`，而 `push_node` 按
path 去重。这些盘仍有唯一 `/dev/vblkN`，但没有唯一 vd 字母名。

`vda1/vda2` 不代表分区：`parse_block_index` 忽略字母后的任意纯数字后缀并返回磁盘 index，
所以 `/dev/vda999` 也会解析到整盘 0。不存在 LBA offset、长度限制或 partition table 对象。
不要用这些路径做破坏性分区测试。

解析器只支持单个小写 ASCII 字母，不支持 Linux 的 `vdaa`。它根据语法计算 index，并不要求
路径已出现在 `DEV_NODES` 快照中。

## 4. 函数语义

| 函数 | 行为 | 注意事项 |
| --- | --- | --- |
| `refresh()` | 按当前 `block_device_count` 清空并重建节点 | 返回 alias 数，不是设备数 |
| `list_nodes()` | 返回缓存的完整副本 | refresh 前为空；可能已陈旧 |
| `lookup_block_device(path)` | 解析 index，再实时 `block_device_at` | 不查询 `DEV_NODES` |
| `default_root_block_path()` | 有至少一设备则返回 `/dev/vda` | 不依赖 refresh |
| `self_test()` | 只检查 index 0/1 命名 | 不验证 lookup 或 registry |

`default_root_block_path` 与 lookup 都直接看实时 driver registry，因此即使忘记 refresh，根路径
仍可能 lookup 成功，而 `list_nodes` 仍为空。这是兼容实现的弱一致性，不可照搬到生产 devfs。

## 5. 调用链

```text
测试/旧调用者
  -> driver 注册 block device
  -> impl_devfs::refresh
     -> block_device_count
     -> 锁 DEV_NODES，clear
     -> 为每个 index 构造 aliases 并去重
  -> list_nodes 用于断言/显示

lookup_block_device(path)
  -> parse_block_index（不取 DEV_NODES 锁）
  -> block_device_at(index)
  -> clone SharedBlockDevice 或 FsError::NotFound
```

lookup 返回 Arc 共享句柄，之后 refresh 不会使该句柄失效。driver 热拔插语义不在此实现中；
registry 若复用 index，旧路径可能解析到新设备。

## 6. 锁与生命周期

唯一内部锁是 `DEV_NODES`。refresh 先在锁外读取一次 count，随后锁内循环构造路径；若 driver
registry 同期增加设备，本轮可能少枚举，下一次 refresh 才出现。lookup 则可能立即看到新设备，
从而产生“可 open 但 list 不显示”。

这里不会在持 DEV_NODES 锁时获取 block device 对象，只读取先前的 count，避免形成明显的
devfs→driver 锁嵌套。但日志宏目前仍在 nodes guard 作用域内；若未来 logging 回入文件系统，
应先保存 len、drop guard，再记录日志。

## 7. 新增真正分区节点实例

不能继续让 `/dev/vda1` 绑定整盘。可靠做法是：

1. driver/block 层定义 `PartitionBlockDevice`，保存父设备 Arc、起始 LBA 和 LBA 数；
2. 在锁外读取 GPT/MBR，并校验签名、整数溢出、范围与 sector size；
3. 为每个合法分区创建独立共享 block handle；
4. devfs 节点保存明确 binding，而非仅靠 path 反推 disk index；
5. read/write 将 partition LBA checked_add 起始偏移，并拒绝越界；
6. refresh 采用局部新快照构造完成后整体 swap，失败保留旧快照；
7. rootfs 明确选择整盘还是分区，不依赖枚举顺序；
8. 测试坏 GPT、重叠/越界分区、4K sector、热刷新和已打开旧 handle。

生产实现已有 binding 表，更适合作为扩展起点。本兼容 crate 若继续保留，应删除虚假的固定
`vda1/vda2`，或清楚地只在测试 fixture 中启用。

## 8. 新增设备类别时的决策

若只是给生产 `/dev` 增加字符设备，不需要同步本 crate，因为它的明确边界是 block-only。
若旧测试必须看见新类别，应先把 `DevNode` 与 lookup 设计成带类型的 binding，而不是给
`DevNodeType::Unsupported` 填路径后仍让 lookup 只返回 block handle。

路径注册必须校验绝对 `/dev/` 前缀、禁止 `..`/NUL、处理类型冲突，并保证目录节点和实际
binding 原子发布。字符串存在不等于设备可打开。

## 9. 常见故障

| 现象 | 原因/首查 |
| --- | --- |
| `list_nodes` 为空但 lookup 成功 | 未 refresh；lookup 直查 driver registry |
| 节点数多于块设备数 | 每盘至少两个 alias，首盘额外两个 |
| 第 27 块没有 `/dev/vd...` 唯一名 | index 截到 z 后去重 |
| `/dev/vda1` 读到整盘 superblock | 当前只是 index 0 alias，无分区偏移 |
| refresh 后旧 handle 仍可用 | Arc 已 clone，符合当前生命周期 |
| 生产 `/dev/null` 修改不生效 | 改错 crate；生产使用 fs-devfs impl-kernel |

## 10. 验证清单

- 0/1/2/26/27 个设备的 alias 数量与唯一性；
- refresh 两次不积累旧节点；
- `/dev/vblkN`、`/dev/vdX`、数字后缀和非法路径解析；
- list snapshot 与并发 registry 增长的已知弱一致性；
- `SharedBlockDevice` alias 指向预期 index；
- 明确测试证明 `vda1/vda2` 当前是整盘 alias，防止误用；
- `self_test` feature、RV/LA `make check` 均通过。

