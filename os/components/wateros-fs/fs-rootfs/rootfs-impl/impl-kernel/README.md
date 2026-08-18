# wateros-fs-rootfs-impl-kernel 离线开发手册

本 crate 保存 WaterOS 当前根卷的全局状态，并把 devfs 块设备路径、活动 `FsImpl` 和 RO/RW
挂载连接起来。它不是 VFS mount namespace：路径路由、per-task root/cwd、子挂载、页缓存与
fd 生命周期仍在 VFS。公共 trait 见 [rootfs-api](../../rootfs-api/api-v0/README.md)，父级
概览见 [fs-rootfs](../../README.md)，通用 FS 契约见
[fs-api](../../../fs-api/api-v0/README.md)。

## 源码地图

| 文件 | 职责 |
| --- | --- |
| `src/registry.rs` | 四个全局槽、`KernelRootFsManager` 的 trait 实现 |
| `src/mount.rs` | 默认/指定路径根挂载、辅助卷挂载、同设备 alias 复用 |
| `src/state.rs` | `MOUNT_GENERATION: AtomicU64` |
| `src/lib.rs` | 模块重导出与公共入口面 |

## 全局状态

| 状态 | 类型 | 含义 |
| --- | --- | --- |
| `ACTIVE_FS_IMPL` | `Mutex<Option<&'static dyn FsImpl>>` | 启动 probe 后选中的后端实现 |
| `ROOT_FS` | `Mutex<Option<SharedFs>>` | 当前 RO 根视图，供 ELF loader/内核读取 |
| `ROOT_RW_FS` | `Mutex<Option<SharedRwFs>>` | 当前 RW 根视图，供 VFS mutation |
| `ROOT_DEV_PATH` | `Mutex<Option<String>>` | 最近成功根挂载所用 devfs 路径 |
| `MOUNT_GENERATION` | `AtomicU64` | mount/cache 视图代次 |

四个 root 槽使用独立 mutex，并不是原子事务。getter 在锁内 clone Arc/String 后立即释放 guard。
任何新代码都不要把 guard 带入后端 mount、日志、VFS 或用户 copy。

`ACTIVE_FS_IMPL` 保存 `'static` 引用，要求聚合注册表中的 `IMPL` 静态实例在整个内核运行期
有效。不要注入栈上或可卸载模块中的对象。

## 初始化与主挂载链

```text
fs::init_after_boot
→ devfs::refresh
→ default_root_block_path
→ registered FsImpl::probe
→ rootfs::set_active_fs_impl(&'static IMPL)

user bring-up
→ fs::mount_default_root_rw
→ rootfs::mount_default_root_rw
→ devfs::default_root_block_path
→ mount_root_rw_from_block_path
→ devfs::lookup_block_device
→ ACTIVE_FS_IMPL（必须为 Some）
→ imp.mount_rw(device)
→ 从 RW 实例构造共享状态的只读适配视图
→ 分别写 ROOT_FS / ROOT_RW_FS / ROOT_DEV_PATH
→ bump_mount_generation
```

为什么 RW 根还要 RO 句柄：VFS 写路径用 `ROOT_RW_FS`，但 ELF loader 当前直接从
`root_fs()` 读取。这里的 RO 句柄是 RW 实例的只读接口适配，两者共享目录项和元数据
缓存；不能对同一块设备再独立 mount 一个 RO 实例，否则写后读可见性无法保证。

所有可失败的查找和 RW mount 都在写全局槽前完成，这避免这些失败留下半状态。
但最终提交仍是三个独立加锁赋值；当前没有并发 root switch 设计。启动期单写者假设下可用，
若支持运行期切换必须改成统一状态对象/事务锁。

RO-only `mount_root_from_block_path` 由 `KernelRootFsManager` 实现，只写 `ROOT_FS` 与路径，不清
既有 `ROOT_RW_FS`。它适用于启动/明确 RO 场景，不应在已有 RW 根上随意调用，否则可能形成
RO/RW 来自不同实例的混合状态。

## mount generation

`mount_generation()` 使用 Acquire load，`bump_mount_generation()` 使用 Release
`fetch_add(1)`。当前 VFS paged handle、stable node、mount table、ELF/mmap 只读页缓存都会把
generation 放进身份或在结构变动后读取它。

正确顺序应是：

```text
阻止/串行化相关访问
→ flush/失效旧 page cache
→ 提交 mount/root 结构变化
→ bump_mount_generation
→ 发布新视图
```

只 bump generation 不会自动 flush 脏页，也不会销毁旧 Arc；它只是让新查找不再把旧缓存键
误当成当前实例。`AtomicU64::fetch_add` 当前会回绕（debug/极长期边界），没有饱和或 epoch
恢复策略。

`next_mount_generation()` 只是未使用的兼容 wrapper，当前编译 warning 即来源于此；新增调用
应优先使用语义更明确的 `bump_mount_generation()`。

## 辅助卷与设备 alias

`mount_aux_ro_from_block_path` / `mount_aux_rw_from_block_path` 返回独立句柄，不替换根槽。为
避免同一块设备被创建两份互不知情的 RW backend/cache，实现会：

1. 解析请求 path 得到 `Arc` block device；
2. 再解析 `ROOT_DEV_PATH`；
3. 用 `Arc::ptr_eq` 比较真实共享对象；
4. 同设备时复用已有 RO/RW 根句柄。

因此 `/dev/vblk0` 与 `/dev/vda` 即使字符串不同，只要 devfs 返回同一个 Arc 就能识别 alias。
不要改为字符串比较。同设备存在 RW 根却没有 RO 根时，aux RO 会拒绝为 `Unsupported`，避免
创建第二个潜在冲突实例。

当前即使复用现有根句柄，辅助挂载函数也会 bump generation，因为 VFS mount view 发生变化。
真正的子 mount 表提交/回滚由 VFS 管理；如果后续把函数拆分，要确保失败的 VFS mount 不会
无意义地发布代次或泄漏新后端。

## 锁序与禁止事项

当前函数经常短暂读取 `ACTIVE_FS_IMPL` guard 后直接调用 `imp.mount_*`。表达式求值会在取得
`&'static dyn FsImpl` 后释放临时 guard，但修改代码时应显式复制到局部并尽快 drop，以免把
注册锁持到块 I/O。

建议锁序/阶段：

```text
短持 rootfs 注册/状态锁，复制 Arc/'static 引用/String
→ 解锁
→ devfs lookup、块 I/O、后端 mount/sync
→ 短持 rootfs 状态锁提交结果
→ 解锁
→ bump generation / 通知上层
```

状态锁内禁止日志、块 I/O、VFS 回调、页缓存写回、等待和用户 copy。不要同时获取多个独立
root 槽锁并调用外部代码；运行期事务化应直接合并为一个 `Mutex<RootState>`。

## clear 与运行期切根风险

`KernelRootFsManager::clear_root_fs` 目前依次清空 RO、RW 和路径，但不：

- sync 后端或 flush VFS 脏页；
- 检查 open fd/mmap/busy mount；
- 清 `ACTIVE_FS_IMPL`；
- bump mount generation；
- 回滚中间并发观察到的半状态。

所以它只适合已经由上层完成生命周期收尾的错误恢复/卸载阶段，不能直接作为 `umount("/")`
实现。若补运行期 unmount/switch-root，应由 VFS 协调并重构为事务状态。

## 增加根选择策略实例

若要按启动参数选择 `/dev/vdb` 而不是默认设备：

1. 在 devfs/启动配置层解析为规范 path；不要在 rootfs 解析用户指针或命令行文本。
2. 聚合层对该 device 运行所有注册 impl 的 probe，并按 RO/RW 需求选择能力匹配者。
3. 调用 `set_active_fs_impl`，再调用 `mount_root_rw_from_block_path(path)`。
4. mount 失败时保留已有根状态；只有全部局部对象成功后才提交。
5. 检查 path alias 指向同一 device 时的重复挂载策略。
6. 记录选中 backend/kind/path，但不要在持 rootfs 锁时 logging。
7. 覆盖无盘、path 不存在、probe miss、RO-only、坏 superblock、重复选择和成功 exec 测试。

多盘自动优先级应该由聚合/策略层定义，rootfs 只执行已经作出的选择。

## 故障定位

| 现象 | 首查 |
| --- | --- |
| 启动提示无根块设备 | devfs refresh、default path、驱动注册 |
| probe 成功但 mount Unsupported | `ACTIVE_FS_IMPL` 是否注入、能力表与 `mount_rw` 是否一致 |
| 文件能写但 exec 报无 rootfs | RW 主路径是否同时成功安装 `ROOT_FS` |
| 切换/挂载后读到旧页 | generation 是否在结构变化后推进、旧 cache 是否 flush/失效 |
| 同盘辅助 RW 后数据损坏 | alias 是否返回同一 block-device Arc、是否绕过复用创建第二后端 |
| mount 失败后根状态混杂 | 是否在所有可失败步骤完成前写了某个全局槽 |
| unmount 后仍有旧实例 | getter 返回的 Arc 仍被 open handle/ELF/VFS 持有，这是生命周期而非槽泄漏 |

## 修改检查清单

- [ ] 所有可失败 mount 对象先在局部构造，成功后再提交。
- [ ] RW 根同时建立 RO/RW 句柄，二者来自同一后端和同一 device。
- [ ] 设备同一性用 Arc/stable identity，不用 path 字符串。
- [ ] root 状态锁内无块 I/O、日志、VFS、用户 copy 或等待。
- [ ] mount 结构变化与 page-cache flush、generation bump 顺序正确。
- [ ] clear/switch 路径处理 open handle、mmap、dirty page 与 sync。
- [ ] 未挂载返回 `None/NotMounted`，未注入后端返回明确错误而非 panic。
- [ ] RV/LA 顶层检查与实际镜像启动均覆盖。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
make shell ARCH=rv PROFILE=final
```

运行期最小验证：正常根能同时 exec 和创建/读回文件；同设备 alias 的辅助 RW 不产生第二份
实例；坏设备挂载失败后旧根仍可用；涉及写回时在镜像副本上退出 guest 后执行只读 fsck 并
重新启动读回数据。
