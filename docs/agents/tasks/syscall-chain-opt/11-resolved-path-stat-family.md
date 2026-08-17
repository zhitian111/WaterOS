# 任务 11：newfstatat/statx 消费 ResolvedPath

## 任务内容与目标

把任务 10 的解析结果用于 newfstatat、statx 和相关 path-stat 入口，避免 symlink 解析后再次
metadata 路由。保持 AT_EMPTY_PATH、AT_SYMLINK_NOFOLLOW、dirfd、权限和 statx mask 语义。

## 实施方案

1. path-stat 公共 helper 返回 `ResolvedPath` 或直接转换其 metadata，不重新按字符串查询。
2. nofollow 使用最终 lstat metadata；follow 使用展开后的节点 metadata。
3. AT_EMPTY_PATH 继续从 fd slot/handle 取 metadata，不强行走路径解析。
4. mount identity 到 dev/inode 字段的转换只做一次；不存在、非目录和权限 errno 保持一致。
5. 增加 symlink follow/nofollow、dirfd、empty path、mount crossing 和并发 rename 测试。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/path_at.rs`
- VFS resolved metadata facade 与测试

## CodeGraph 查询

```bash
codegraph explore "sys_newfstatat sys_statx resolve_path_at metadata"
codegraph impact "sys_newfstatat"
codegraph callers "resolve_path_at"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# LTP stat/statx/fstatat 与 symlink/dirfd 定向回归
cd .. && git diff --check
```

任务 01 计数确认 path-stat 最终节点不重复 lookup/getattr；BuildStorm A/B 中 stat/open 阶段无
功能回退并记录墙钟变化。

## Commit 与简报

提交建议：`[perf] stat 家族复用路径解析结果`。新增 `history/11-brief.md`。
