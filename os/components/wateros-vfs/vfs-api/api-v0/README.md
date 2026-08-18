# VFS API v0 开发手册

[VFS 总览](../../README.md) · [离线开发手册](../../../../../docs/offline-development/README.md)

本 crate 是 syscall/task/MM 与具体 VFS/FS 实现之间的稳定契约。它定义路径、元数据、打开句柄、fd 会话、挂载和错误，但不依赖 `wateros-fs`，也不返回 Linux `-errno`。

## 文件与边界

| 文件 | 主要契约 |
| --- | --- |
| `backend.rs` | `VfsBackend`：根卷能力、打开与元数据操作入口。 |
| `handle.rs` | `VfsIoHandle`、prepared read、OFD state、文件内容身份、设备映射。 |
| `fd.rs` | `VfsFdSession` 和标准 fd 编号。 |
| `path.rs`、`resolve.rs` | 绝对路径规范化、cwd 合成、symlink 跟随规则。 |
| `mount.rs`、`namespace.rs` | mount 操作和只读 mount table 视图。 |
| `meta.rs`、`kind.rs`、`dev.rs` | 节点类型、元数据、能力和设备清单。 |
| `rw_session.rs`、`root_read.rs` | bring-up 等受限根卷读写视图。 |

## 三种不能混淆的对象

1. 路径名是 namespace 中的目录项，可 rename/unlink，也可经 symlink 指向别处。
2. 稳定节点身份是 `(mount_generation, mount_id, node_id, content version)`；打开后即使路径删除，句柄仍可持有节点。
3. open file description（OFD）保存共享 offset/status flags。一次 `open` 新建 OFD，`dup` 与普通 fork 共享它；`FD_CLOEXEC` 属于 fd slot，不属于 OFD。

因此，不能以路径字符串代替已打开文件的永久身份，也不能为 dup 后每个 fd 各自保存 offset。

## prepared read 契约

顺序读先通过 `VfsOpenDescriptionState::begin_read` 预留当前 offset；句柄把数据准备到内核对象，用户拷贝结束后才以实际 copied 字节调用 finish。若用户页故障或 syscall 取消，则 cancel，offset 不前进。同一 OFD 同时只允许一个 active reservation，其它读返回 `Busy`。

```text
prepare_read(max_len)
  -> begin_read: 捕获 offset + reservation id
  -> 后端读取/等待，fd 表锁不应长时间持有
  -> copy_to_user 可能部分成功
  -> finish(copied <= staged) 或 cancel
  -> 仅 committed 字节推进共享 offset
```

新增流式句柄时必须实现相同的“消费提交”语义，否则 pipe/tty/socket 在 EFAULT 后会丢数据。

## 路径与设备映射

所有公共绝对路径先 `normalize_absolute_path`；解析相对 symlink 时以链接所在目录为基准，展开上限 40，最终组件是否跟随由 `FinalSymlink` 决定。进程 root 限制由实现层保证，`..` 不能逃出虚拟 root。

`VfsDeviceMapping` 只携带物理范围和保活 lease，MM 映射期间 lease 必须存在；解除用户映射时不能把设备页交给普通帧分配器。framebuffer 区域先用 `fits` 检查非空、边界和坐标溢出。

## 扩展一个新句柄操作

1. 在 `VfsIoHandle` 添加可安全默认 `Unsupported` 的方法，并说明 offset、阻塞、部分成功和所有权。
2. 更新普通文件、目录、pipe/tty/字符设备、proc/sys、O_PATH 等相关实现；不支持者明确返回错误。
3. 若操作会改变内容，推进 `VfsFileContentIdentity` version 并维护页缓存一致性。
4. syscall 层完成用户结构和 errno 转换，VFS API 不引用 Linux ABI 常量。
5. 测试 close/dup/fork/exec、并发 I/O、unlink-open、用户拷贝部分失败和非阻塞行为。
