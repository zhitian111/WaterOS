# K-05C another-ext4 写回合并优化报告

```text
task: K-05C another-ext4 flush batching
date: 2026-07-31
kernel_commit: 83ce4608d0b5b9c1674893cc2c5bb153b2c4d136 + 本报告对应未提交修复
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: RISC-V64, 8 CPU
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256: 2f35982605e8afab7940743bd88281968c6e78985d71326f9808bde8d3d330ab
overlay: 每轮使用基于干净 os/sdcard-rv.img 的新 qcow2 overlay
commands: make rv_check; make la_check; timeout 30s qemu-system-riscv64 ...; qemu-img convert; e2fsck -fn
result_markers: EXT4_FLUSH_BENCH_DONE; PERSIST_REPEAT_COMPARE=OK
first_failure: none
raw_log_path: 未保留；target 后续构建清理了短测日志，原始值记录于本报告
raw_log_sha256: unavailable
```

## 结论

已消除 another-ext4 文件页写回中的重复全缓存 flush。修改没有改变文件系统公共 API、
task 模块或调度器；同步边界从每个连续页段下沉写入后一次 flush，调整为一次文件脏页
写回完成后统一同步后端。

## 问题

页缓存会把连续脏页合并为写回段，但旧路径在每段写入后都刷新 another-ext4 的全部
内部块缓存：

```text
PagedFileHandle::sync_dirty()
  -> page cache flush()
  -> FsPageIo::write_range()
  -> AnotherExt4Fs::write_range()
  -> Ext4::write()
  -> Ext4::flush_all()
```

文件存在多个不连续脏页段时，同一次 `fsync` 或 close 会重复扫描并刷新后端缓存，放大
块设备 I/O 和锁持有时间。

## 修改

- `AnotherExt4Fs::write_range()` 只把数据写入 another-ext4 缓存，不再逐段
  `flush_all()`。
- `PagedFileHandle::sync_dirty()` 在页缓存完成该文件全部脏页写回后，仅执行一次对应
  文件系统的 `sync()`；没有脏页时避免无意义同步。
- `reset_file_page_cache()` 在丢弃全局页缓存前显式同步 root 文件系统，保留重置路径
  的持久化语义。
- 创建、删除、重命名和 truncate 等元数据路径仍沿用原有即时 flush 行为。

## 性能验证

测试在相同内核配置和干净镜像的独立 overlay 上，将 1,937,944 字节的
`/glibc/busybox` 连续写入目标文件 8 次，共 15,503,552 字节，再执行 `sync`。

| 版本 | 第 1 轮 | 第 2 轮 | 第 3 轮 | 中位数 |
| --- | ---: | ---: | ---: | ---: |
| 修改前 | 5.079 s | 5.036 s | 5.192 s | 5.079 s |
| 修改后 | 4.957 s | 4.980 s | 4.932 s | 4.957 s |

修改后中位数降低约 2.40%，三轮结果均低于修改前的三轮结果。收益不大但方向稳定，
同时减少了多核写回时后端全局锁和块缓存 flush 的频率。

## 完整性验证

- `make rv_check`：通过。
- `make la_check`：通过。
- 将修改后 overlay 转换为 raw 镜像后，目标文件大小为 15,503,552 字节。
- 将目标文件按 1,937,944 字节分成 8 段，每段与源 busybox 逐字节比较，结果为
  `PERSIST_REPEAT_COMPARE=OK`。
- `e2fsck -fn` 完成五阶段检查，无文件系统错误。
- 临时 bringup 命令已还原，测试文件未加入仓库。

本次遵循白天短时验证约束，没有运行完整 iozone、BuildStorm、pre 或 final 测试。
K-05 的压力测试和断电/失败注入回归应在夜间全量测试窗口继续执行。
