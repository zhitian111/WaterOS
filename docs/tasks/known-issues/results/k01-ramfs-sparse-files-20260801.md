# K-01 ramfs 稀疏文件修复结果（2026-08-01）

## 问题与根因

初赛镜像中的 LTP `rename01` 通过测试设备框架在 `/tmp` 创建并 `fallocate` 默认
300 MiB 镜像。`/tmp` 使用 `impl-ramfs`，原实现以连续 `Vec<u8>` 表示文件；仅扩展
逻辑长度也会实际分配 314572800 字节，触发内核堆分配失败。该问题与 ext4 镜像内容
无关，也不在 fs-bridge 的 cgroup tmpfs 路径中。

## 实现

- 普通文件改用 4 KiB 页索引的稀疏存储，文件逻辑长度与实际分配页数分离。
- `truncate` 扩展只更新长度；洞区读取为零；收缩会删除越界页并清零保留页尾部。
- 写入只为包含非零数据的页分配内存，全零页会被回收。
- ramfs 容量限制按实际分配页计费，并在分配前检查容量。
- symlink 仍使用内联字节数组，目录和特殊节点语义未改变。

涉及文件：

- `os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs`

## 验证

- ramfs host 单测：4/4 通过，覆盖 300 MiB 稀疏扩展、跨页写、收缩后再扩展和容量计费。
- `make check`：通过。
- `make la_check`：通过。
- 30 秒定向 RISC-V QEMU：不再出现 300 MiB 堆分配失败或 panic；`rename01` 已推进到
  `tst_device.c`，随后因 `No free devices found` 退出。

日志：`/tmp/wateros-ramfs-sparse-latest-rename01-20260801.log`。

## 剩余问题

本次没有宣称 `rename01` 通过。WaterOS 当前缺少 LTP mount-device 用例需要的可用
loop/test block device；需要作为独立任务评估 loop 设备、相关 ioctl 和挂载链路。
按照白天测试约束，本次未运行完整 LTP、BuildStorm、iozone 或决赛套件。
