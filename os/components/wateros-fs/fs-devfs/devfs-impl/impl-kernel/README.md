# wateros-fs-devfs-impl-kernel 离线开发手册

本 crate 把 block/character driver 全局注册表投影成 WaterOS 当前 `/dev` 节点快照和路径绑定。
公共接口见 [devfs-api](../../devfs-api/api-v0/README.md)，父级设计见
[fs-devfs](../../README.md)。VFS 通过这份状态枚举 `/dev` 并构造设备 handle，rootfs 通过它
选择/查找根块设备。

## 源码地图

| 文件 | 职责 |
| --- | --- |
| `src/manager.rs` | `DEVFS` 状态、refresh、注册/查找、默认根策略 |
| `src/aliases.rs` | Linux 风格块/字符 alias 去重与绑定 |
| `src/fs_impl.rs` | devfs 的 `FsImpl` 能力占位注册项 |
| `src/lib.rs` | 依赖导入和模块重导出 |

## 核心状态

```text
DEVFS: Mutex<DevFsImpl>
DevFsImpl:
  nodes: Vec<DevNode>
  block_bindings: Vec<(String, SharedBlockDevice)>
  character_bindings: Vec<(String, SharedCharacterDevice)>
  dt_unsupported_paths: Vec<String>
```

`nodes` 是目录视图，bindings 是实际 I/O 查找表，必须理解为两套相关但不完全相同的数据：
VFS 内建的 `/dev/zero` 等可以只有节点；DTB unsupported 只能有占位；真实驱动 alias 通常
两边都有。

当前使用 Vec 线性查找，适合少量启动设备。若改为 map，需要保持稳定枚举顺序或显式排序，
否则默认根 fallback 和用户态 `readdir` 顺序会变化。

## `refresh` 的精确调用链

```text
platform driver init/register
→ machine devfs refresh 或 fs::init_after_boot
→ block_device_count + block_device_at 收集局部快照
→ character_device_count/at/kind_at 收集局部快照
→ 短锁 DEVFS，clone dt_unsupported_paths
→ 再锁 DEVFS，清 nodes/block_bindings/character_bindings
→ 生成所有 aliases
→ 加入内建特殊字符节点
→ 合并 DTB unsupported 节点
→ 日志报告 total/block/character/unsupported
```

先收集 driver 快照再锁 DEVFS，避免持 devfs 锁遍历 driver registry。注意当前末尾
`logging::info!` 仍发生在 DEVFS guard 生命周期内；若 logger 或未来 console 路径回入 VFS/devfs，
可能形成锁序风险。修改时建议先复制统计、显式 drop guard，再 logging。

refresh 会清掉手工 `register_*_device` 加入的 binding，除非设备也存在 driver registry 并由
别名生成重新加入。需要持久的动态注册时，应把“额外注册源”单独保存并在 refresh 合并，而
不是假定当前 bindings 永久存在。

## 当前别名规则

### 块设备

设备索引 `idx` 生成：

- `/dev/vblk{idx}`；
- `/dev/vd<letter>`，0 为 `vda`，25 为 `vdz`；
- idx 0 额外生成 `/dev/vda1`、`/dev/vda2`。

这些别名都 clone 同一个 `SharedBlockDevice`。`vda1/vda2` 没有解析 GPT/MBR、没有 LBA offset，
不是独立 partition device。超过 25 的索引都被 `.min(25)` 截成 `vdz`，`push_block_alias` 又按
路径去重，因此第 27 块起会缺少唯一 Linux 字母 alias（但仍有 `/dev/vblkN`）。扩展设备数时
需要实现 `vdaa/vdab...` 或只使用唯一编号命名。

`push_block_alias`/`push_char_alias` 发现路径已存在就直接返回，不替换旧 handle；而公开
`register_*_device` 对已存在 path 会替换 binding。两条路径冲突语义不同，新增代码必须选择
预期行为。

### 字符设备

- 所有 registry 项先有 `/dev/ttyS{idx}`，无论 kind；
- idx 0 额外映射 `/dev/console` 与 `/dev/tty`；
- `Rtc` 增加 `/dev/misc/rtc`、`/dev/rtc0`、`/dev/rtc`；
- `Null` 增加 `/dev/null`；
- 若缺少对应节点，再补 `/dev/null`、`/dev/zero`、`/dev/urandom`、
  `/dev/cpu_dma_latency` 的 Character 目录项。

“所有字符设备都有 ttyS alias”只是当前简化逻辑；增加非 serial 设备时应考虑是否继续暴露
误导 alias。内建特殊节点没有 binding 时由 VFS special handle 处理，不能直接调用
`lookup_character_device(...).unwrap()`。

## 查找、注册与生命周期

`lookup_*` 在 DEVFS 锁内线性查 path，clone Arc 后返回。refresh 清空表不会使已经 clone 给
open fd/rootfs 的设备对象失效；它只影响后续按路径查找。设备热拔插的真实失效、I/O 错误和
引用回收属于 driver API，devfs 没有 hotplug state machine。

公开 register 对同路径替换 binding，但不会修正已存在 `nodes` 的错误 node_type。例如一个
Unsupported path 后来注册块设备，当前逻辑会在 bindings 新增并再 push 一个 Block node，
产生重复路径。若实现驱动动态绑定，应按 path 原子更新/去重 nodes，而不是只 append。

路径未做统一合法性校验；调用者应传 `/dev/...` 绝对路径。若开放动态注册，必须拒绝空路径、
非 `/dev` 前缀、`..`、NUL 和重复类型冲突。

## 默认根设备

```text
在 nodes 中精确找 /dev/vda
→ 找不到则取第一项 DevNodeType::Block
→ clone path；无块节点为 None
```

因为 `/dev/vda` 当前总是第 0 块盘的 alias，所以正常情况选设备 0。fallback 顺序取决于
nodes 构造顺序。该函数不验证 binding 是否存在；保持节点/binding 一致是必要不变量，否则
rootfs 下一步 lookup 会 NotFound。

## `FsImpl` 占位的特殊性

`KernelDevFsImpl` 在 FS 注册表声明 `(DevFs, ReadOnly)`，用于能力展示；但 `mount_ro` 永远
返回 `Unsupported`，也不参与块卷 probe。真实 `/dev` 是 VFS bridge 对 devfs manager 快照的
视图，不是通过 `FsImpl::mount_ro(block_device)` 建立的卷。

不要因为 capability 表写了 ReadOnly 就在 rootfs 选择它挂块设备；聚合 probe 默认返回
`Ok(None)`，所以正常不会匹配。

## 添加动态设备的可靠路线

1. driver registry 先发布共享设备和 kind。
2. refresh 在锁外取得完整 snapshot。
3. 在局部 `DevFsImpl` 构造新 nodes/bindings，处理 path 冲突与排序。
4. 短持 DEVFS 锁整体 `replace`，避免读者看到清空到半重建的中间状态。
5. 解锁后 logging，并通知/推进 VFS 需要的目录缓存 generation。
6. 已打开 handle 继续持有旧 Arc；新 open 使用新快照。
7. 测试 register 与 refresh 竞争、移除、替换、重复路径和 open-then-refresh。

当前实现是锁内 clear+重建，锁外读者无法看见半状态，但临界区包含 Vec 分配和日志。整体局部
构造再 swap 能缩短锁时间，也能自然处理失败回滚。

## 故障定位

| 现象 | 首查 |
| --- | --- |
| `ls /dev` 有节点但 open NotFound | 节点是否仅占位/内建；对应 binding 是否存在 |
| rootfs 报 lookup block NotFound | default path 是否有一致 block binding；是否 refresh 后手工绑定被清除 |
| `/dev/vda1` 数据与整盘相同 | 当前就是 alias，没有分区偏移 |
| 多于 26 块盘 alias 丢失 | `linux_vd_disk_path` 截到 z 且路径去重 |
| RTC 被当 tty 暴露 | 当前所有 character registry 项先生成 ttyS；检查 kind 命名策略 |
| refresh 后已有 fd 仍能 I/O | fd 持有 Arc，符合当前生命周期；不是节点表泄漏 |
| 动态绑定出现重复目录项 | register 没有按 path 更新既有 Unsupported/其它类型 node |
| devfs/driver 并发死锁 | 检查是否持 DEVFS 锁访问 driver registry 或执行日志/VFS 回调 |

## 修改检查清单

- [ ] driver registry 快照在取得 DEVFS 写锁前完成。
- [ ] 真实节点、binding、node type 对同一路径保持一致。
- [ ] alias 去重不把不同设备静默合并；超过 26 盘仍有唯一名称。
- [ ] 分区路径只有存在独立 partition device 时才宣称分区语义。
- [ ] refresh 对动态额外注册的保留/清除策略明确。
- [ ] DEVFS 锁内无设备 I/O、VFS 回调、用户 copy；日志最好在解锁后。
- [ ] default root path 一定能 lookup 到 block handle。
- [ ] open fd 在 refresh/移除后的 Arc 生命周期经过测试。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

启动后对照 driver registry count 与 devfs 日志；逐项验证 `/dev` readdir/stat/open，并用
`Arc::ptr_eq` 或设备 identity 检查所有预期 alias。根设备验证必须继续跑 probe、mount、exec，
不能止于 `default_root_block_path()` 返回字符串。
