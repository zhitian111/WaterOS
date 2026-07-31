# K-01 阶段结果：大写入短写语义

## 问题与根因

BuildStorm 在编译 `libc v0.2.186` 时稳定失败：

```text
error: failed to write .../rustcenRzB9/lib.rmeta: Invalid argument (os error 22)
error: could not compile `libc` (lib) due to 1 previous error
```

`sys_write()` 把 `SYSCALL_IO_MAX`（4 MiB 内核暂存区上限）当成 syscall ABI 上限，
请求超过 4 MiB 时直接返回 `EINVAL`。这与 Linux 的大 I/O 语义不符，也使 rustc
无法处理较大的元数据写入。

## 修改结果

- `write` 和 `pwrite64` 对大请求最多暂存 4 MiB，并返回合法短写。
- `writev` 和 `pwritev` 对 iovec 前缀执行相同的有界短写。
- iovec 描述符地址使用 checked arithmetic，避免地址计算溢出。
- 所有新增暂存区使用 fallible allocation，失败返回 `ENOMEM`。
- 增加小请求保持不变和大请求被限制到暂存上限的单元测试。
- 未修改 task API、调度器、任务资源或生命周期代码。

## 验证记录

基线镜像为 `os/sdcard-rv-pub.img`，SHA-256：

```text
dd9bbc442f990b228087f15c8da14776981eb38ee393a84a89daf39e46c119d0
```

- `make rv_check`：通过。
- `make la_check`：通过。
- `git diff --check`：通过。
- 独立 crate `cargo test --lib` 无法链接：未选择 platform feature，已有的
  `ArchTimeImpl/ArchInterruptImpl/ArchPagingImpl` 类型不存在；不是本次源码错误。
- RISC-V64 QEMU、OpenSBI、8 CPU、8 GiB、qcow2 overlay：
  `BUILDSTORM_TOOLCHAIN ok`、`BUILDSTORM_MINIBUILD ok`。
- `libc` 编译后已开始编译依赖它的 `errno v0.3.14` 和 `mio v1.2.1`，日志中无
  `Invalid argument`，确认越过原失败点。
- 默认 QEMU 在串行工具链探测阶段约占用一个宿主 CPU，进入 Cargo 并行编译后升至
  约四个宿主 CPU；不能把启动阶段的低占用误判为所有 vCPU 均被单线程串行模拟。

## 验收边界

本次未声称 K-01 完成：BuildStorm 尚未输出最终 compile 成功标记。QEMU 在活跃编译
时由外部终止，随后 `e2fsck -fn` 发现未完成 extent，因此该 overlay 不能作为
文件系统完整性通过证据。后续必须使用全新 overlay 跑完整流程并在受控同步后复检。
