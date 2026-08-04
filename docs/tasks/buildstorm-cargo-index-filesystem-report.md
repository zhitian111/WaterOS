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

## 确认结论

**故障已定位并修复。** 根因不在镜像和 ext4 磁盘结构，而在 WaterOS 的
`read(2)` 系统调用：用户请求长度超过 4 MiB 时，内核错误返回 `EINVAL`。Cargo
使用 `std::fs::read` 按元数据大小读取 `4,324,435` 字节的 `web-sys` 索引，恰好
超过该阈值，因此将缓存视为不可读并执行失效删除。

使用重新下载的干净 `sdcard-rv-pub.img` 和独立 qcow2 overlay 后，问题仍可稳定
复现，因此可以排除镜像制作遗漏。原始 raw 镜像在测试前后的 SHA-256 均为
`c43d184dc8eda5dfcd6a07eda3d2468c4bda8076a25004ae91d4724b5251f8bb`，
未被测试修改。

## 根因与修复

问题代码位于：

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`

原实现将内核临时缓冲上限错误地当成用户 ABI 限制：

```rust
if len > SYSCALL_IO_MAX {
    return UserRet::from_error(ErrNo::EINVAL);
}
```

`SYSCALL_IO_MAX` 为 4 MiB。Linux 允许 `read` 返回少于 `count` 的字节数，因此修复
保留 4 MiB 内核分配上限，但把更大的请求限制为一次最多传输 4 MiB 并返回短读。
用户态标准库随后继续读取剩余数据。

## 已确认事实

- `/work/tgoskits/Cargo.lock` 锁定 `web-sys 0.3.103`。
- 镜像中存在
  `/root/.cargo/registry/cache/.../web-sys-0.3.103.crate`。
- 镜像中存在
  `/root/.cargo/registry/src/.../web-sys-0.3.103/`。
- 干净镜像中 Cargo 解析所需的 sparse-index 路径
  `/root/.cargo/registry/index/.../.cache/we/b-/web-sys`
  是 inode `266673`、大小 `4,324,435` 字节的普通文件。
- `os/scripts/rv_final_run.sh` 直接以可写 raw 设备挂载
  `os/sdcard-rv-pub.img`，重复测试会修改基准镜像。

## 复现与证据

1. 从宿主机 Cargo 缓存取得 `web-sys` 索引，大小为 `4,324,435` 字节，
   SHA-256 为
   `cb2cd5f44e7ec7e7ce91bafe75ea73d7b6c880e65dfe13ef716f9de1a560762f`。
2. 将该文件写入镜像副本后，`debugfs` 可正常读取，`e2fsck -fn` 五阶段检查通过。
3. WaterOS 启动后，在执行 Cargo 前由 guest 运行 `wc` 和 `sha256sum`，得到完全相同
   的大小与校验和，排除了镜像内容错误和普通顺序读取错误。
4. 执行 `cargo metadata --offline` 时，目标路径的诊断日志记录到两次明确的
   `unlink` 请求；随后文件变为 inode 0 的已删除目录项，Cargo 报告找不到
   `web-sys`。
5. 宿主机 Cargo 使用同一索引执行离线解析时已成功解析 `web-sys 0.3.103`，
   之后才因未缓存的 `bumpalo` 停止，证明该索引内容和格式本身有效。
6. 测试后的 overlay 仍通过 `e2fsck -fn`。inode 0 是 ext4 删除目录项的正常表示，
   不能单独作为磁盘结构损坏的证据。
7. 在重新下载的镜像上再次执行完整 BuildStorm：`BUILDSTORM_TOOLCHAIN` 和
   `BUILDSTORM_MINIBUILD` 均通过，`tg-xtask` 与正式构建都在解析 `web-sys` 时
   失败。测试后的 overlay 中该路径已变成 inode 0，而测试前的原始镜像中仍是
   inode `266673`；这直接证明删除发生在 WaterOS 运行期间。
8. 修复后，BuildStorm 不再报告 `web-sys` 缺失，`tg-xtask` 已进入 `446` 个单元的
   实际编译阶段。
9. 在全新 overlay 上执行真实 `/work/tgoskits` 的
   `cargo metadata --offline --format-version 1`，返回码为 `0`，生成
   `3,925,224` 字节 JSON；随后检查 `web-sys` 路径仍存在。
10. 修复后的 BuildStorm overlay 中索引仍为 inode `266673`，大小和 SHA-256 与
    基线一致，且 `e2fsck -fn` 五阶段检查通过。

因此此前观察到的 inode 0 是 Cargo 在读取失败后的主动删除结果，不是
another-ext4 错误删除了其他路径，也不是镜像预先缺少该文件。

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

该步骤仍用于保护基线和验证最终修复，但不再阻塞当前内核侧定位。

## 后续验证

截至 2026-08-04，双架构完整 BuildStorm 均已输出
`BUILDSTORM_COMPILE mode=multi ok=true`。历史 `fsync fd=6` 已确认是目录 fd，并由
句柄能力分派修复；当前正式日志不再出现该警告。完整持久化闭环及日志见
[`known-issues/results/k01-final-20260804.md`](./known-issues/results/k01-final-20260804.md)。
剩余工作是 K-04/K-10 的性能基线与最终候选冻结，不再属于 Cargo 索引正确性问题。

## 验收标准

- [x] 干净基准镜像保持只读，测试使用独立 overlay。
- [x] `cargo metadata --offline` 成功，`web-sys` 索引保持有效。
- [x] `make rv_check` 通过。
- [x] 运行后的文件系统通过 `e2fsck -fn` 五阶段检查。
- [x] 完整输出 `BUILDSTORM_COMPILE mode=multi ok=true`。
- [x] CAgent 10/10 和初赛用例无新增回归。
