# 任务 02 简报：mount namespace Arc/COW 快照

## 完成状态

已完成。per-task registry 与 bootstrap namespace 均保存 `Arc<MountNamespace>`；路径路由只在
锁内克隆 `Arc`，mount/umount 等写入口通过 `Arc::make_mut` 在首次修改时分离快照。

## 提交

本简报与 `[perf] mount namespace 使用 Arc COW 快照` 实现位于同一提交。

## 关键文件与行为

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_ns.rs`
  - registry 的 namespace 槽改为 `Arc`。
  - copy 只复制 `Arc`，首次写入 COW；share 仍复用同一 owner 槽。
  - 修正 owner 自身 unshare 时其余共享成员的 owner 重挂与引用计数。
  - 增加 copy/share/unshare/drop 的数据模型测试。
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
  - route 和 bootstrap 返回 `Arc` 快照。
  - bootstrap 写入口统一使用 COW helper。

普通 route 不再深拷贝 mount entries、mount point/source 字符串或下层 FS `Arc`。

## 验证

通过：

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
cd ..
git diff --check
```

局部 host 测试命令未能进入本 crate 测试：

```bash
cd os
cargo test --offline --manifest-path \
  components/wateros-vfs/vfs-impl/impl-fs-bridge/Cargo.toml
```

失败点是 host 配置未选择 `wateros-platform-arch` 的 `ArchTimeImpl`、`ArchInterruptImpl` 和
`ArchPagingImpl`，与本次改动无关。新增测试已由两架构内核构建完成类型检查，但尚未在 host
test harness 中执行。

## 性能与剩余风险

任务 00 runner 与任务 01 计数尚未实现，因此本次未生成 namespace deep-clone 计数或
BuildStorm 交错 A/B。运行期 mount/procfs、`CLONE_NEWNS` 与 unshare 回归仍需在最终 QEMU
门禁中执行。若出现共享 mount 可见性或 namespace 隔离回归，应回退本提交。

## 文档同步

本次未改变公开命令、feature 或目录结构；除本任务简报外无需同步用户文档。
