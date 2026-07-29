# 阶段 0：兼容性收口

## 阶段目标

在开始性能优化前，证明官方 BuildStorm、CAgent 和初赛用例的功能链路完整。阶段出口
不是“已经开始编译”，而是正式脚本输出成功标记并且运行后文件系统结构正常。

## C0-1 固化大文件短读修复

**负责人：A**

涉及：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`
- `docs/tasks/read-family/README.md`

任务：

- 保留 4 MiB 内核临时缓冲上限，但让更大的 `read(2)` 返回合法短读。
- 核对 `pread64`、`readv` 和其它读取入口是否存在同类“实现上限变成 ABI 错误”的
  行为；只修有真实调用证据的入口。
- 按 `docs/tasks/read-family/README.md` 收口访问模式、EFAULT 数据消费、OFD 共享状态
  和向量/定位读取；这些是已确认正确性问题，不再仅作为性能风险记录。
- 用全新 qcow2 overlay 重跑 `cargo metadata --offline`，确认 `web-sys` 索引大小和
  SHA-256 不变。

验收：`make rv_check`、`make kernel-rv-final` 通过，定向 Cargo metadata 返回 0。

## C0-2 闭环 fsync/fdatasync 语义

**负责人：A**

当前 `sync_fd()` 在调用 `flush()` 前拒绝所有非 `File` 节点，BuildStorm 日志出现
`fd=6` 的 `Unsupported`。不能直接把所有 fd 改为成功。

任务：

1. 增加临时、定向诊断，记录 task、fd、节点类型、可用时的路径和调用类型。
2. 确认 fd=6 是普通文件、目录、管道、socket 还是特殊节点。
3. 对照该类型的 Linux 行为决定返回值；普通文件必须走真实写回，目录是否支持应由
   VFS/文件系统能力表达，管道和 socket 不能伪造持久化成功。
4. 补最小回归测试，删除临时高频日志。

重点文件：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/sync.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{file_handle,dir_handle,paged_handle}.rs`

验收：正式 BuildStorm 不再出现未解释的 fsync 警告，错误类型与 Linux 语义一致。

## C0-3 验证文件时间戳

**负责人：A；B 协助提供稳定进程测试**

当前 `FsMetadata`/`VfsMetadata` 没有持久化时间字段，`LinuxStat` 默认时间为 0，
`stat_times.rs` 仅保存本次启动中的 syscall 层覆盖值。这可能导致 Cargo 增量判定
异常，但目前只是风险，不能在没有实验前直接扩展公共 API。

按以下顺序验证：

- 同一次启动连续执行两次相同小型 Cargo build，记录第二次是否复用产物。
- 重启后保留 target 再执行，记录 Cargo 的 dirty reason。
- 比较 guest 中源文件、产物和目录的 `stat/statx` 时间。
- 只有确认时间戳导致错误重编或语义失败后，才把 atime/mtime/ctime 扩展到
  `FsMetadata`、`VfsMetadata` 和 another-ext4 映射。

验收：形成“无需修改”或“已修复并有定向测试”的明确结论。

## C0-4 完整功能验收

**负责人：A 集成；B、C 回归各自主责模块**

- [ ] 全新 overlay 输出 `BUILDSTORM_TOOLCHAIN ok`。
- [ ] 输出 `BUILDSTORM_MINIBUILD ok`。
- [ ] 输出 `BUILDSTORM_COMPILE mode=multi ok=true`，产物大小满足评测要求。
- [ ] CAgent 连续三轮 10/10，无超时、panic 和网络请求缺失。
- [ ] 初赛 basic/busybox 关键用例无新增回归。
- [ ] overlay 转 raw 后通过 `e2fsck -fn` 五阶段检查。
- [ ] 原始镜像测试前后 SHA-256 不变。

全部勾选后才能进入阶段 1。若完整编译失败，按第一个 syscall、fault 或等待对象回到
本阶段修复，不把“运行慢”当作失败原因。
