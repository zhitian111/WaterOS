# T07：ext4/block-cache/VFS 锁拆分

## 任务内容

这是性能能否成立的关键一步。目标是消除「跨 I/O 持锁」：

```text
SharedRwFs spin lock
  -> another_ext4 全局 block-cache spin lock
    -> SharedBlockDevice spin lock
      -> virtio add_notify_wait_pop busy poll
```

要求 miss 时释放缓存/FS 锁，提交请求并睡眠，唤醒后回查再装填；绝不在持有
`spin` 锁的情况下 sleep 或切换任务。

## 实施方案

1. 块缓存：把元数据/数据缓存锁与 backend 调用分离；miss 释放锁 → 读 backend →
   重取锁 → 二次检查后装填。
2. `another_ext4`（vendor 补丁，需明确理由）的 block-cache 锁同样在 backend read 前释放并回查。
3. `SharedRwFs` 从互斥锁改为读写锁（或等价），读侧并发；变更操作保持独占。
4. 先做纯重构不引入 IRQ，用同步 backend 验证锁语义正确；T06 的异步路径再叠加。
5. 按 `os/components/wateros-vfs/README.md` 已有 lock ordering 文档更新锁序说明。

## 涉及文件

- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/{lib,device,manager}.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/vendor/another_ext4/src/ext4_defs/cache.rs`（vendor 补丁，谨慎）
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{lib,paged_handle,mount_table,path_ops}.rs`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`（`SharedRwFs`/`SharedFs` 类型）

## CodeGraph 查询命令

```bash
codegraph explore "SharedRwFs LocalRwFs ReadWriteFs SharedBlockDevice"
codegraph impact SharedRwFs
codegraph explore "CachingBlockDevice read_blocks admit_read_miss"
codegraph explore "FsPageIo read_range write_range"
```

## 实施备注与方案细化（T07-a 之后）

### 顺序依赖

- **先修 T06 用户态死锁，再做 T07-c 的任务睡眠收益**：T06 的 IRQ 路径当前在
  用户态（cagent 模型加载）最终死锁（5 核自旋、无读 stall 警告），与锁拆分无关；
  不修好它，T07-c 让出的睡眠窗口没有可用对象。T07-c 前的 QEMU 调试优先做 T06。
- T07-a（块缓存锁拆分）已完成并提交（`0860b957`），当前 FS 串行下行为等价。

### T07-b：another_ext4 块缓存锁释放 + 回查（vendor 补丁）

- 目标：`another_ext4`（vendor）内部 block-cache 锁在 backend read 前释放，
  读回后重取锁回查再装填；与 T07-a 对称。
- 风险：vendor 补丁，需先在副本上验证；根因必须是 vendor 缓存锁跨 I/O，否则
  不改。

### T07-c：SharedRwFs 读写锁 + FS 读侧并发

1. `fs-api/api-v0/src/handles.rs`：`SharedRwFs = Arc<Mutex<LocalRwFs>>` →
   `Arc<RwLock<LocalRwFs>>`（`spin` 启用 `rwlock` feature）；`SharedFs` 视需要同步。
2. `ReadWriteFs` 读操作（`metadata_node`/`read_range_node`/`getxattr`/`is_mounted`）
   已是 `&self`，可直接用 read guard；写操作保持 write guard。
3. 调用点分类规则：
   - 只调 `&self` 方法 → `.read()`；
   - 调任何 `&mut` 方法（含 `open_node`/`close_node`/写路径）→ `.write()`；
   - `open_node` 用 write guard 取节点，后续按节点读取用 read guard。
4. 涉及面：约 39 个文件、约 400 处 `.lock()`；`vfs-bridge`（`lib/paged_handle/
   mount_table/path_ops/stable_node`）、`fs-rootfs`、`fs-devfs`、syscall 层。
5. 锁序文档：`os/components/wateros-vfs/README.md` 的 lock ordering 段落同步更新。
6. 风险与停止条件：历史实验（`perf/virtio-multi-outstanding-irq`）在本步出现
   cagent 卡死；出现死锁/停滞/数据损坏即回退到 `Mutex` 并记录证据，不做无界
   调试。
7. 验证：宿主单测 + `rv_check`/`la_check` + QEMU（cagent + BuildStorm），QEMU
   在后台无运行实例时执行。

## 验收方式

- 静态：双架构 check。
- 单测：块缓存并发 miss 回查与一致性 host test；another_ext4 `--features block_cache` test。
- QEMU 功能：双架构 rootfs 挂载、读写、重开、fsync/unmount 后 `e2fsck -fn`（镜像副本）无错。
- 重点观察无死锁、无饥饿、无数据损坏。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# 用镜像副本 + overlay 验证，避免污染基准镜像
```

## 任务简报

完成后写 `history/07-lock-split-ext4-vfs.md`。
