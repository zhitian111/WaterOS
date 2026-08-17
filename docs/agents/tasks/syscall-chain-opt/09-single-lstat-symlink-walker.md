# 任务 09：symlink walker 每个分量只做一次 lstat

## 任务内容与目标

普通无 symlink 路径的每个分量只做一次 lookup/metadata；仅当该次 lstat 的 node type 是
symlink 时才 readlink。保持 40 层限制、最终分量 follow/no-follow、chroot、绝对/相对链接、
proc magic link 和 `..` 语义。

## 实施方案

1. walker 通过任务 08 token API检查 node type，不再把 `read_symlink(NotAFile)` 当类型探测。
2. 中间分量用同一 metadata 校验 Directory；symlink 通过 token 按 inode 读取目标。
3. 以 deque/index 替代 `Vec::remove(0)`，避免路径深度带来的 O(d²) 搬移。
4. 路由使用任务 02 的同一 namespace Arc 快照，整个解析期间 mount identity 一致。
5. 增加相对/绝对/循环/断链/final nofollow/chroot escape/mount boundary 测试。

## 涉及文件

- `os/components/wateros-vfs/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{path_ops,mount_table}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/path_at.rs`
- VFS self-test

## CodeGraph 查询

```bash
codegraph explore "resolve_symlink_absolute resolve_symlink_in_root_absolute read_symlink_path"
codegraph impact "resolve_symlink_absolute"
codegraph callers "resolve_symlinks"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-vfs/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
# symlink/openat2/chroot/procfs 定向用户回归
cd .. && git diff --check
```

任务 01 计数证明深度 d 的无 symlink 路径不再执行 d 次 readlink 探测；所有 symlink 语义测试
保持一致。使用任务 00 runner做 BuildStorm A/B。

## Commit 与简报

提交建议：`[perf] symlink walker 复用单次 lstat`。新增 `history/09-brief.md`。
