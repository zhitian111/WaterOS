# 任务 05 简报：FD slot 分类与单次 I/O 快照

## 完成情况

- FD 表内部统一为 `FdSlot`，handle、descriptor flags、资源分类和终端 ID 不再由平行表维护。
- `VfsResourceKind` 覆盖普通文件、目录、pipe、网络 socket、TTY、Unix socket、epoll 和其它
  资源；分类在安装 FD slot 时确定并缓存。
- dup、dup3、fork、`CLONE_FILES`、unshare、close-range 与 close-on-exec 均保留正确的分类
  和 flags 语义。
- unshare 的 owner 迁移会重建 `open_counts/free_fds`，避免迁移后 nofile 计数和最低空闲 FD
  索引仍留在旧 owner；self-test 覆盖 owner 与两个共享者的迁移。
- 新增 `FdIoLease`，一次全局 registry 查询同时返回稳定 handle、flags 和资源类型。
- `read`、`readv` 与 `pread64` 已改为在 syscall 入口取得一次 lease；权限检查、`O_PATH`、
  TTY 作业控制、socket 等待与实际 I/O 不再重复进入全局 FD registry。
- VFS self-test 增加安装、dup、fork 与 `CLONE_FILES` 的分类/flags 断言。

## 主要文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-network/src/socket/fd.rs`
- 普通文件、目录、memfd、Unix socket、epoll 与 pipe/TTY 句柄实现

## 验证记录

```text
make rv_check          PASS
make la_check          PASS
make kernel-rv-final   PASS
git diff --check       PASS

cargo test --offline --manifest-path \
  components/wateros-vfs/vfs-impl/impl-fd-session/Cargo.toml
                        BLOCKED（未选择 platform-arch 实现，`ArchTimeImpl`、
                        `ArchInterruptImpl`、`ArchPagingImpl` 未定义）
```

自定义 QEMU 可执行文件版本已核对为 9.2.1；完整运行证据见下节。

## RISC-V 8 核性能回归

后续在实现提交 `cad80778e1fa39f32567dc37b15413feb666c32c` 上完成一轮 final 镜像回归：

```text
QEMU                    9.2.1
SMP / memory            8 / 16 GiB
snapshot                enabled
kernel SHA-256          6b3ecde9e7fa45a9eeee8c8498b5e03c798fd141752b085ef01273464479f233
image SHA-256           dfb2b31b30118aab37fcf39be905bbc22584baf84512ca1e5eb3b88247b88ff6
log SHA-256             42e7540cfb33aec51ff2c8ee1aa4256250684c7ac92b1e61d1f003229de0b75e
QEMU return code        0
cagent                   exit_code=0, elapsed=2.239s
BuildStorm               status=OK, rc=0, cores=8, elapsed_s=571.39, run=OK
BuildStorm wrapper       exit_code=0, elapsed=610.614s
completion marker        all commands finished
panic/OOM/FS error scan  no matches
```

日志：`/home/zhitian/project/WaterOS_perf_results/syscall-chain-opt/syscall-chain-opt-perf-20260817T031052Z.log`。
运行前后镜像 SHA-256 一致，
确认 QEMU `-snapshot` 没有改写基准镜像。该结果证明本提交未引入可观察的完整 workload
功能回退；由于任务 00 的交错 A/B runner 尚未实现，单轮结果不能用于宣称确定的性能收益。

## 限制与剩余风险

- 任务 00/01 的性能 runner 与计数基线尚未实现，因此本提交没有交错 BuildStorm A/B 数据。
- 性能回归使用 QEMU 9.2.1 和 `-snapshot`，未对基准镜像执行写入。
- `preadv` 仍使用兼容路径；本次验收范围为 `read`、`readv` 与 `pread64`。
