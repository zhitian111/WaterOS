# K-05D ramfs 物理页 payload

## 结果摘要

ramfs 普通文件 payload 已由内核堆 `Vec<u8>` 迁移到不可复制的
`OwnedPhysPage`。目录树、页索引、xattr 等元数据仍使用内核堆。稀疏 hole 不分配页，
全零页和 truncate 删除的页通过 RAII 回收；页耗尽或 mount resident 限额超出均返回
`FsError::NoSpace`。

文件实体改为 `Arc<Mutex<Inode>>`，hardlink 共享同一 inode 和物理页。稳定 node handle
持有 open 引用，因此 unlink/rename 后 fd 仍可访问原 inode，最后一次 close 才释放 payload。
所有公开操作仍由 `SharedRwFs` 外层锁串行；锁顺序为 ramfs 外层锁 -> inode 锁 -> frame
allocator 锁，代码不在这些临界区调用设备 I/O 或调度等待。

bootstrap `/tmp` 的 resident payload 上限设为 512 MiB；用户态 tmpfs 的 `size=` 仍按挂载
实例生效。正式 BuildStorm 工程位于 ext4 `/work`，只有最小工具链探针位于
`/tmp/minibuild`。

## 修改范围

- `impl-ramfs/src/lib.rs`：物理页稀疏文件、共享 inode、稳定 node I/O、容量计费和自检。
- `impl-ramfs/Cargo.toml`、`os/Cargo.lock`：依赖 frame allocator aggregate。
- `base-config/src/fs.rs`、`impl-fs-bridge/src/mount_table.rs`：bootstrap tmpfs 上限。
- `wateros-fs/{src/lib.rs,fs-api/api-v0/src/lib.rs}`：更新 ramfs 后端说明。

依赖图为 `fs -> frame-allocator -> mm-api/platform API`，未引入 `fs -> mm-impl` 或循环依赖。

## 验证记录

```text
task: K-05D
date: 2026-08-04
kernel_commit: 0ad6627a + 本报告所在提交
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64, 8 CPUs
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256_pre: eed7f895f54a0a606d8bf05e2558650dd51f3b02b74b9703f6ad6fb1e8f03516
image_sha256_final: e4912bf0084dd53bb7eae99a1d2e61311a8fcf823b6ec1a761c7317c33d84fe2
overlay: QEMU -snapshot
commands: make rv_check; make la_check; 90s rv_pre_run; 120s rv_final_run
raw_log_path_pre: /tmp/wateros-results/k05d/rv-pre-bulk-20260804.log
raw_log_sha256_pre: 837d4aa26603ca687bb2b7461a860934dc4ec7cc345881903a4953705d3a7964
raw_log_path_final: /tmp/wateros-results/k05d/rv-final-short-20260804.log
raw_log_sha256_final: 0fe3dd2dd017320f9522c644e167a67bc7d589644c4be7d5e98c780c31b3e4eb
first_failure: final 限时结束于正式 BuildStorm 编译，未观察到 ramfs/allocator 错误
```

- `make rv_check`、`make la_check` 通过；仅有仓库既有 warning。
- kernel self-test 覆盖 300 MiB sparse truncate/hole 读零、跨页/尾部清零、hardlink
  共享写、ENOSPC 后原数据保持、unlink-open-fd 和最终 close 回收。
- 临时 129 MiB 实写测试写入 135266304 字节、33024 页：free frames
  `226858 -> 193834 -> 226858`，首尾内容正确；临时代码已删除。
- pre 8 核启动后继续运行 cyclictest/LTP，无 panic、OOM、double free 或页回收错误。
- final 8 核中 CAgent 10/10 通过；`BUILDSTORM_TOOLCHAIN ok`、
  `BUILDSTORM_MINIBUILD ok`，完成 `tg-xtask` 并进入正式多核编译。

裸机 target 无 Rust `test` crate/global allocator，`cargo check --tests` 无法使用；实际页访问和
Drop 由 kernel self-test/QEMU frame 计数验证。120 秒 final 仅为白天限时回归，不代表完整
BuildStorm 已通过；LoongArch 本轮只做编译检查。

## 调试工具

后续若压力测试卡住，使用 `os/scripts/wateros_debug.py watch/snapshot` 区分 VFS、frame
allocator 锁等待和正常编译推进。当前 `doctor` 的 QEMU、RISC-V binutils 检查通过，但主机
缺少名为 `gdb-multiarch` 的命令；安装前不能生成自动完整 GDB 报告。
