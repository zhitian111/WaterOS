# `prctl05` 线程名与 `/proc` comm（2026-08-09）

## 问题

LTP `prctl05` 中 `PR_SET_NAME/PR_GET_NAME` 已正常，但：

```text
Failed to open FILE '/proc/self/task/5/comm' for reading: ENOENT
```

procfs 没有实现 `/proc/<pid>/task/<tid>/comm`，顶层 `/proc/<pid>/comm`
也不存在；即使存在，原来也只从 argv/exe 推导，不使用 `PR_SET_NAME` 设置的
线程名。

## 修改

`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs`：

- 新增 `/proc/<pid>/comm`、`/proc/<pid>/task/<tid>` 与
  `/proc/<pid>/task/<tid>/comm`。
- `comm_for(pid)` 优先返回 leader 线程的 `thread_comm`，再回退 argv/exe。
- `/proc/<pid>/task/<tid>/comm` 读取对应线程的 comm，并补 `\n`。
- `/proc/<pid>/task` 目录可枚举线程，`/proc/<pid>/task/<tid>` 目录包含
  `comm` 文件。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/prctl05-fixed2.log`：

- `PR_SET_NAME/PR_GET_NAME` TPASS
- `/proc/self/task/5/comm` TPASS
- `/proc/self/comm` TPASS
- 长名截断场景全部 TPASS

LoongArch LTP 定向日志 `/tmp/prctl05-la-fixed.log` 同样全部通过。
