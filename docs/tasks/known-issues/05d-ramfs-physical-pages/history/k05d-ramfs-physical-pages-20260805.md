# K-05D `ramfs` 物理页 payload 收口结果（2026-08-05）

## 结论

`impl-ramfs` 的 payload 已使用不可复制的物理页持有者（`OwnedPhysPage`）实现，`/tmp`
采用 `BOOTSTRAP_TMPFS_LIMIT_BYTES=512 MiB` 限额，空洞不计费；该子任务在
`2026-08-05` 的决赛验证链路中处于闭环状态。当前双架构最终 run 中未观察到
`ENOSPC` 以外的 ramfs/payload 异常，且没有触发 kernel heap OOM 或 panic。

代码范围保持不变：页负载不在 heap 内部直接复制，open/reopen 共享 inode 及句柄生命周期，
`truncate`/`truncate(2)`、unlinks 与 hardlink 语义已与既有结果文件一致；`ENOSPC`
由容量校验路径统一返回。

## 任务文件与验收依据

- `os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs`
- `os/components/wateros-fs/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
- `os/components/wateros-base/base-config/src/fs.rs`
- `docs/tasks/history/known-issues/k05d-ramfs-physical-pages-20260804.md`（基础回归）

## 验收与命令

```text
date=2026-08-05
kernel_commit=1898f408fe25abb3d9baf39104e2f56468fc22ae
user_submodule_commit=2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture=riscv64+loongarch64, WOS_SMP=8, booted ramfs bootstrap
qemu_version=11.0.2
openSBI=1.7
command_rv="timeout 2400s taskset -c 0,2,4,6,8,10,12,14 env WOS_QEMU_SNAPSHOT=1 bash ./scripts/rv_final_run.sh"
command_la="timeout 2100s taskset -c 16-23 env WOS_QEMU_SNAPSHOT=1 bash ./scripts/la_final_run.sh"
```

`riscv` 与 `loongarch` 均完成：

- `CAgent 10/10`
- `BUILDSTORM_TOOLCHAIN ok`
- `BUILDSTORM_MINIBUILD ok`
- `BUILDSTORM_COMPILE mode=multi ok=true`

## 关键判定

- [x] `/tmp` payload 使用物理页存储，不再走内核堆 `Vec<u8>` 分配。
- [x] `/tmp` 引导限额 512 MiB 已配置并生效。
- [x] 无全局 OOM 或 `BadAddress` 类似内存异常报告。
- [x] `kagent + BuildStorm` 的双架构决赛链路未卡死，不影响后续 K-10 全量验收流程。
- [ ] pre/busybox/LTP 仍按上层路线图按阶段补充；K-10 的 `pre/busybox/LTP/final` 全链仍需在该路径下独立汇总。
