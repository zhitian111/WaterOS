# BuildStorm Cargo 离线索引文件系统问题报告

## 问题概述

RISC-V64、QEMU `virt`、8 核、8 GiB 环境运行
`/glibc/buildstorm_testcode.sh` 时，工具链和最小构建均通过：

```text
BUILDSTORM_TOOLCHAIN ok
BUILDSTORM_MINIBUILD ok
```

正式构建在 Cargo 离线依赖解析阶段失败：

```text
error: no matching package named `web-sys` found
required by package `reqwest v0.13.4`
```

脚本设置的 `HOME=/root`、`CARGO_HOME=/root/.cargo` 正确；
`CARGO_NET_OFFLINE=true` 是评测要求，不能通过联网规避。

## 已确认事实

- `/work/tgoskits/Cargo.lock` 锁定 `web-sys 0.3.103`。
- 镜像中存在
  `/root/.cargo/registry/cache/.../web-sys-0.3.103.crate`。
- 镜像中存在
  `/root/.cargo/registry/src/.../web-sys-0.3.103/`。
- Cargo 解析所需的 sparse-index 路径
  `/root/.cargo/registry/index/.../.cache/we/b-/web-sys`
  当前是 inode 为 0、大小为 0 的已删除目录项。
- `os/scripts/rv_final_run.sh` 直接以可写 raw 设备挂载
  `os/sdcard-rv-pub.img`，重复测试会修改基准镜像。

因此当前直接阻断是“crate 和源码存在，但版本索引不可查”。现有证据尚不能区分：

1. 原始镜像打包时就缺少索引；
2. 基准镜像被此前的直接可写测试修改；
3. WaterOS 的 rename/unlink、目录项或页缓存一致性错误删除了索引。

## P0：建立可复现基线

负责人首先取得未启动过的官方镜像，只读保存，并为每轮测试创建独立 qcow2 overlay。
分别在启动前、Cargo minibuild 后、tg-xtask 后和正式构建后检查 sparse-index。

宿主机检查示例：

```bash
debugfs -R \
  'stat /root/.cargo/registry/index/index.crates.io-1949cf8c6b5b557f/.cache/we/b-/web-sys' \
  sdcard-rv-pub.img
e2fsck -fn overlay-expanded.img
```

若启动前已缺失，应归类为镜像制作问题；若仅在 WaterOS 运行后消失，再进入以下内核
修复任务。

## P1：目录项和原子替换

检查 Cargo 临时文件的 `create -> write -> fsync -> rename` 及旧文件 `unlink`
链路，重点确认同目录 rename、覆盖已有目标和失败回滚不会产生 inode 0 的可见项。

涉及文件：

- `os/vendor/another_ext4/src/ext4/high_level.rs`
- `os/vendor/another_ext4/src/ext4/low_level.rs`
- `os/vendor/another_ext4/src/ext4/link.rs`
- `os/vendor/another_ext4/src/ext4_defs/dir.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`

增加针对“覆盖 rename 后立即 lookup/read、旧名消失、新名存在”的宿主回归测试，并在
8 核下并发重复执行。

## P2：页缓存与持久化一致性

确认 rename/unlink 前脏页已刷回，成功后旧、新路径缓存均正确失效；禁止已删除文件
的延迟 close/flush 覆盖新文件。检查错误淘汰是否会把无关文件的回写错误传播给索引
读取。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/vendor/another_ext4/src/ext4_defs/cache.rs`

## P3：大索引读取与并发访问

对 sparse-index 文件执行完整 `stat/read/mmap/close` 校验，比较读取字节数和校验和。
并行运行多个 Cargo 解析进程，确认 lookup 不出现瞬时 `ENOENT`、短读或旧缓存命中。
诊断日志仅按目标路径采样，避免日志量影响时序。

## 验收标准

- 干净基准镜像保持只读，三轮测试均使用新 overlay。
- 每个阶段的 `web-sys` index inode、大小和校验和保持有效。
- `cargo build -p tg-xtask` 在离线模式下成功。
- 正式输出 `BUILDSTORM_COMPILE mode=multi ok=true`。
- 运行后的文件系统通过 `e2fsck -fn` 五阶段检查。
- CAgent 10/10 和初赛用例无新增卡死、panic 或文件损坏。

