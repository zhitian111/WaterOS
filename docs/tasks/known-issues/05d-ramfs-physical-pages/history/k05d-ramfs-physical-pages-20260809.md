# K-05D ramfs 物理页与 tmpfs 容量复验（2026-08-09）

## 结论

K-05D 的核心实现已经进入当前 `main`，本次复验确认 `/tmp` 的 payload 使用物理页
而非内核堆，容量限额与帧回收均有效：

- `OwnedPhysPage` 由 [`0ad6627a`] 引入。
- `SparseFile.pages` 切换到物理页由 [`dc26172d`] 完成。
- bootstrap `/tmp` 限额为 `BOOTSTRAP_TMPFS_LIMIT_BYTES = 512 MiB`。

本次没有新增内核代码，只补充了基于当前 `main`（`77559994`）的运行证据。

## 测试环境

```text
arch=riscv64
profile=pre（必须使用 pre：final profile 下 /tmp 仍是镜像普通目录）
image=os/sdcard-rv.img 的 reflink 副本
smp=8
memory=8G
snapshot=1
taskset=0,2,4,6,8,10,12,14
```

测试通过交互 shell 执行：

```bash
make shell ARCH=rv PROFILE=pre SDCARD=/tmp/sdcard-rv-subreaper.img \
  GUEST_SHELL=/bin/sh
```

## 实测数据

### 基线

```text
MemTotal: 909500 kB
MemFree:  905936 kB
```

### 小文件非零写入

```bash
dd if=/glibc/busybox of=/tmp/big bs=1M count=4
```

实际写入 1,937,944 字节后：

```text
MemFree: 904040 kB
```

物理页占用约 1.8 MiB；`rm` 后恢复：

```text
MemFree: 905936 kB
```

### 128 MiB 非零写入

```bash
dd if=/dev/urandom of=/tmp/big bs=1M count=128
```

写入后：

```text
MemFree: 774864 kB
```

`MemFree` 下降 131,072 kB，正好等于 128 MiB；无 kernel heap OOM 或 panic。
删除后恢复 `905936 kB`。

### 超过限额

```bash
dd if=/dev/urandom of=/tmp/big bs=1M count=600
```

输出：

```text
dd: error writing '/tmp/big': No space left on device
545+0 records in
544+0 records out
570425344 bytes (544.0MB) copied
```

文件最终大小为 536,870,912 字节（512 MiB），`MemFree` 降至 381,636 kB，
删除后恢复基线。限额路径返回 `ENOSPC`，没有 panic。

## 边界说明

达到限额时日志会出现 `[paged_handle] close writeback failed ... err=NoSpace` 与
`[vfs-fd] drop_task_fd_table ... close failed: NoSpace`。这是 page-cache 在关闭时
把脏页刷到 ramfs 的失败传播，用户态仍稳定收到 `ENOSPC`；后续可考虑把容量校验提前到
page-cache 写入路径，减少关闭时刷新的告警噪音，但不阻塞 K-05D 的资源正确性结论。

## 代码覆盖

`os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs` 的运行时自测覆盖：

- 300 MiB sparse truncate 后 `resident_pages=0`，hole 读零。
- 跨页写、shrink 后 grow 不暴露旧尾部。
- 容量限额按已分配页而非空洞计费。
- hardlink/unlink/open-node 生命周期结束后帧回收。

启动日志对应：

```text
[ramfs-test] frames_before=226835 frames_after=226835 resident_pages=0
```

## 相关文件

- `os/components/wateros-mm/mm-frame-alloctor/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
- `os/components/wateros-base/base-config/src/fs.rs`
