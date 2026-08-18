# wateros-fs-rootfs-api-v0 离线开发手册

本 crate 定义最小的根卷管理契约 `RootFsManager`。它只管理当前 RO 根句柄与根设备路径；
活动后端选择、RW 根句柄、devfs 查找、挂载代次和全局锁属于
[`impl-kernel`](../../rootfs-impl/impl-kernel/README.md)。父模块说明见
[fs-rootfs](../../README.md)，底层 FS 类型见 [fs-api](../../../fs-api/api-v0/README.md)。

## trait 方法语义

| 方法 | 语义 | 调用者责任 |
| --- | --- | --- |
| `set_root_fs` | 安装一个已成功挂载的 `SharedFs` | 不应传入半初始化句柄 |
| `root_fs` | 克隆并返回共享 RO 句柄 | `None` 必须作为未挂载处理，不能 unwrap |
| `clear_root_fs` | 清除根句柄及实现维护的关联路径 | 上层先处理 open handle、页缓存、写回与 mount table |
| `mount_root_from_block_path` | devfs path → block device → 活动 `FsImpl::mount_ro` → 安装 | path 是内核字符串，不是用户指针 |
| `current_root_device_path` | 返回最近成功根挂载的路径副本 | 路径 alias 不等于设备 identity |

`SharedFs` 是 `Arc<Mutex<LocalFs>>`。`root_fs()` 克隆的是 Arc，不复制文件系统实例；清除全局
槽也不会强制销毁仍被 ELF loader/VFS 等持有的 Arc。这正是 unmount/switch-root 必须由更高层
协调引用生命周期的原因。

## RO 与 RW 根的边界

本 v0 trait 只描述 RO 根，内核实现另行维护 `SharedRwFs`。当前 bring-up 的 RW 主路径会同时
调用同一 `FsImpl` 的 `mount_ro(device.clone())` 和 `mount_rw(device)`：

```text
RO 句柄 → ELF loader、内核只读路径
RW 句柄 → VFS mutation、用户文件写入
```

只安装 RW 会使依赖 `root_fs()` 的 ELF 加载失败；只安装 RO 则写 syscall 无可用后端。未来
若把 RW 管理加入 API，应一次性设计“根视图整体提交/清除”，不要增加彼此独立、容易产生半
状态的 setter。

## 挂载调用链

```text
wateros-fs::init_after_boot
→ devfs 选默认块设备并 probe FsImpl
→ impl-kernel::set_active_fs_impl

bring-up
→ mount_default_root[_rw]
→ devfs::lookup_block_device
→ FsImpl::mount_ro[/mount_rw]
→ RootFsManager::set_root_fs / 实现层整体提交
→ bump_mount_generation
```

API 不规定文件系统格式，也不做 probe。调用 `mount_root_from_block_path` 前必须已经注入活动
`FsImpl`；否则当前实现返回 `FsError::Unsupported`。

## 实现或扩展检查清单

- [ ] getter 返回共享句柄 clone，不泄露全局 mutex guard。
- [ ] 所有可失败的设备查找和 mount 尽量在提交全局槽之前完成。
- [ ] 清除/切换根之前，VFS 已拒绝新访问并完成页缓存写回、open handle 和 mount 清理。
- [ ] 根设备用 block-device identity 判断，不能只比较路径字符串。
- [ ] 成功挂载/切换后由实现推进 mount generation，使旧缓存键失效。
- [ ] `None`/`NotMounted`/`Unsupported` 在调用链中有明确区分。

## 新增 switch-root API 的实例路线

不要直接新增 `set_root_fs(new)` 然后在 syscall 中调用。完整流程应是：

1. VFS 验证目标 mount、权限和 busy 引用；阻止新的旧根 mutation。
2. 写回旧根所有页缓存，再调用旧 RW 后端 `sync`。
3. 在局部变量中构造新 RO/RW 句柄与设备 identity。
4. 实现层用单一事务/统一锁提交新句柄与路径。
5. 推进 mount generation，并使旧 ELF/mmap/page-cache key 不再命中新根。
6. 最后释放旧全局 Arc；仍被合法 open handle 持有的实例按生命周期延迟销毁。
7. 测试挂载失败回滚、busy、同设备 alias、open fd、mmap 和重启持久性。

若 API 需要承载该操作，应定义“完整根状态”结构或事务方法，而不是暴露四个独立 setter。

## 验证

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

运行期至少覆盖无块设备、未注入 impl、坏 superblock、正常 RO/RW 根、重复挂载及挂载失败后旧
根仍可用。

