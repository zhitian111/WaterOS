# T07-c 任务简报：SharedRwFs 改读写锁支持读并发

- 完成日期：2026-08-15
- commit：`02fd912f`（`[refactor] SharedRwFs 改读写锁支持读并发（T07-c）`）
- 前置：`0860b957`（T07-a 块缓存锁拆分）

## 实际改动摘要

- `fs-api`：`SharedRwFs = Arc<Mutex<LocalRwFs>>` → `Arc<RwLock<LocalRwFs>>`
  （`spin` 启用 `rwlock` feature）；`LocalRwFs` 补 `unsafe impl Sync`（写经
  RwLock 独占、读方法 `&self` 由具体 FS 实现保证线程安全）。
- `SharedFs` 保持 `Mutex`（只读卷热路径不改，收窄本次改动面）。
- 全部 `SharedRwFs` 调用点转换（集中在 vfs-bridge，47 处）：
  - 读路径（`read_range/read_dir/metadata/read_symlink/exists/read`、节点
    `metadata_node/read_range_node/getxattr/listxattr`）→ `.read()`；
  - 写/变更路径（`sync/write_range/truncate/mkdir/unlink/rename/open_node/
    close_node/link_node/create_tmpfile_node` 等）→ `.write()`。
- 5 个 `mount_rw` 构造点（another-ext4 / ext4 / ext4-rs / ramfs / tmpfs bootstrap）
  改为 `Arc::new(RwLock::new(...))`。

## 验证

- `make rv_check` / `make la_check`：通过。
- `make kernel-rv-final` / `make kernel-la-final`：构建成功。
- `git diff --check`：干净（61+/61- 对称改动）。
- 编译期强校验：`.read()` 守卫只能调用 `&self` 方法，分类错误会直接编译失败，
  因此本次 47 处分类均通过编译验证。

## 未验证项 / 风险 / 下一步

- **QEMU 回归未跑**（后台用户 QEMU 运行中，按规则不并发启动）：本改动首次允许
  并发读，需 cagent/FS 读写回归确认无数据竞争/时序回归；窗口空闲时补跑。
- 锁序文档（`os/components/wateros-vfs/README.md` 的 lock ordering）需随本次改动
  复核：`SharedRwFs` 由互斥改为读写锁后，读守卫可并发、写守卫独占，原有「根卷
  `SharedRwFs`」在锁序中的角色不变（仍是最外层 FS 锁）。
- 下一步：T07-b（`another_ext4` 块缓存锁释放 + 回查，vendor 补丁），随后把
  T06 的 IRQ 完成路径从「自旋等待」升级为「任务睡眠」（本改动为其提供结构前提），
  并重新评估启用。
