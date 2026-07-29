# K-03C：busybox kill、mv、rmdir 0 分

## 任务目标

独立复验 busybox `kill 10`、`mv test_dir test` 和 `rmdir test` 历史 0 分，分离
signal 与文件系统副作用，修复当前仍存在的 Linux 语义错误。

## 执行前必读

- `docs/tasks/known-issues/03-functional-zero-scores.md`
- `docs/prompts/general.md`
- `docs/prompts/debug_workflow.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-fs.md`

## 已知信息与代码证据

root layout 已避免创建名为 `test` 的 applet 链接。目录测试必须从干净状态执行：

```sh
mkdir test_dir
mv test_dir test
test -d test
rmdir test
test ! -e test
```

`mv` 失败会让后续 `rmdir test` 连锁失败，所以三项不能只看总脚本得分。

## 涉及文件

- `os/src/user_bringup_root_layout.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/renameat2.rs`
- `os/components/wateros-vfs/`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/`
- `test_case/` 中 busybox 脚本

## 任务内容

1. `kill` 单独检查目标存在/不存在、signal 0、权限与 errno。
2. `mv` 从全新目录运行，检查 rename 前后 inode/type/父目录项。
3. `rmdir` 分别验证空目录、非空目录、不存在路径和当前工作目录相关 errno。
4. 每项与 Linux 对照；修复放在 syscall、VFS 或 FS 的实际责任层。
5. FS 变更使用 overlay，结束后检查 ext4。

## 如何验收

- [ ] 三项分别通过，不依赖前一项残留状态。
- [ ] rename/rmdir/link/unlink LTP 子集和 root layout 无回归。
- [ ] signal/kill LTP 子集通过。
- [ ] `make rv_check && make la_check` 及 `e2fsck -fn` 通过。

交付 `docs/tasks/known-issues/results/k03c-YYYYMMDD.md`。
