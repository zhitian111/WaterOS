# fs-procfs

[返回 wateros-fs](../README.md) · [task](../../wateros-task/README.md) · [MM](../../wateros-mm/README.md) · [VFS](../../wateros-vfs/README.md)

procfs 是按需生成的只读内核视图。它拥有路径到伪节点的映射和 Linux 风格文本/二进制格式，不拥有 task、MM、VFS 或 IPC 的业务状态。跨组件数据通过窄函数指针回调注入。

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `procfs-api/api-v0/src/lib.rs` | `ProcFsView`、目录项/元数据和 callback 类型 |
| `path.rs` | 相对 `/proc` 路径解析为 `ProcNamespace`/节点 |
| `view.rs` | exists/metadata/read/read_range/readlink/read_dir 分发 |
| `render.rs` | status/stat/maps/meminfo/mounts 等格式化 |
| `callbacks.rs` | task/VFS/timer/SysV IPC 回调注册与短调用边界 |
| `fs_impl.rs` | 把 procfs 声明成可注册的只读 `FsImpl` |

## 调用链

```text
启动时状态所有者 register_*_lookup(fn)
  -> VFS 挂 /proc
  -> open/read /proc/path
  -> VFS ProcFileHandle
  -> KernelProcFs::read_range
  -> path parse + task/PID resolution
  -> callback 复制稳定快照
  -> render Linux 格式
  -> 按 offset/len 返回
```

callback registry 使用 `Mutex<Option<fn>>`。查询必须先复制函数指针、释放 callback 锁，再调用外部组件；不能持 registry 锁跨组件调用。状态所有者也应在自己的锁内只复制快照，解锁后格式化 Vec/String。

## 动态进程竞态

`/proc/<pid>` 在 exists、open、read 之间可能退出。实现必须接受：

- 路径解析时存在、读取时消失：返回 NotFound；
- 已取得完整快照后进程退出：可返回这份自洽快照；
- task 与 PID leader 映射变化：重新验证 leader，不保存裸引用。

不要持 task registry 借用跨越字符串格式化或用户 copy。maps/smaps 数据应来自 MM 的值类型 snapshot，并明确 file path/permission/shared/private。

## 新增节点

1. 在 API 定义最窄 snapshot/callback 类型，避免 procfs 依赖具体实现 crate。
2. 在 `callbacks.rs` 增加 register 和 query，未注册行为明确为空/0/NotFound。
3. 在 `path.rs` 增加静态或 PID 节点解析。
4. `view.rs` 的 exists、metadata、read/read_range、readlink、read_dir 保持一致。
5. `render.rs` 使用 Linux 期望单位、字段顺序和换行。
6. 在对应状态初始化后、挂 procfs 前注册 callback。

大文件（maps、mounts、sysvipc）应优先覆盖 `read_range`，避免每次小 read 都重新分配完整内容；但分段读取必须保证 offset 语义稳定。

## 回归

测试根目录枚举、普通文件、symlink、PID 不存在、进程并发退出、offset/EOF/短 buffer、未注册 callback 和 SMP 并发读。`/proc/meminfo` 的 MemTotal 不等于固定内核 heap，文档与工具不可混用这两个指标。

snapshot或格式化分配失败必须返回可诊断错误，不能在持task/MM/VFS锁时构造大文本；读取中对象消失应按节点语义给EOF或NotFound，禁止悬垂引用。
