# K-01 another-ext4 块 I/O 错误传播（2026-08-05）

## 问题

`another_ext4::BlockDevice` 的读写接口不返回 `Result`。WaterOS 适配层此前在块大小
不兼容或 virtio-blk 读写失败时直接 `panic!`，压力测试中的瞬时后端错误会导致整个
内核退出，也无法由 VFS 按 `EIO` 处理。

## 修改

- 在 `BlockAdapter` 与 `AnotherExt4Fs` 之间共享原子 I/O 错误闩锁。
- 块设备失败时记录块号和驱动错误，并停止 panic。
- mount、read、write、元数据更新及 flush 在操作前后检查闩锁；本次后端调用即使
  因 trait 限制返回了占位块，也不能再向 VFS 误报成功。
- mount 使用局部 `Ext4`，只有加载过程没有块 I/O 错误时才发布已挂载状态。
- remount 会创建新的错误状态，避免旧设备错误永久阻止重新挂载。

修改局限在 `fs-impl/impl-another-ext4`，未改变 FS 公共契约，也未修改 task 模块。

## 验证

```text
cargo test --manifest-path os/components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
test result: ok. 4 passed; 0 failed

cd os && make rv_check
Finished release profile; RISC-V check complete

cd os && make la_check
Finished release profile; LoongArch64 check complete
```

RISC-V 初赛镜像使用 2 vCPU、2 GiB 和 snapshot 运行 30 分钟：完成 glibc LTP，进入
`libcbench-glibc` 后由外层超时终止。期间 rootfs 持续可读写，没有 another-ext4
panic 或块后端 I/O 错误。该运行是长流程存活验证，不代表初赛全通过；日志仍有
epoll、exec 和 waitpid 的 LTP 语义失败，需按对应任务继续处理。

## 结论

another-ext4 后端块 I/O 失败现在会稳定上报 `FsError::Io`，不会直接 panic，也不会
在同一次成功返回路径中静默吞掉错误。正常双架构构建和现有单元测试通过。
