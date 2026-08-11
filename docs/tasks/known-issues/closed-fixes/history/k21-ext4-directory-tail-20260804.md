# ext4 索引目录线性化 tail 修复与 RIO-10 双架构回归

## 问题定位

RIO-10 LoongArch 初赛回归运行后，`e2fsck -fn` 报告：

```text
Directory inode 264, block #0: directory has no checksum.
Directory inode 131327, block #0: directory has no checksum.
```

对应路径为 `/musl/ltp/testcases/bin` 和 `/glibc/ltp/testcases/bin`。root-layout 在删除
不适配的 LTP 项目时会把 ext4 htree 目录转为线性目录。`DirBlock::ensure_tail()` 原先只
检查块末 12 字节是否具有 tail 的字节特征；旧 `..` 目录项的 `rec_len` 仍覆盖到 4096，
使该 tail 实际位于普通目录项内部，标准工具因而认为 checksum tail 不存在。

## 修复

删除不可靠的末尾字节快速返回。目录线性化时始终遍历目录项链；若最后一个普通目录项
越过 `BLOCK_SIZE - 12`，先缩短其 `rec_len`，再写入独立的 checksum tail。修改仅涉及：

- `os/vendor/another_ext4/src/ext4_defs/dir.rs`

新增单测构造 `..` 覆盖整个 4096 字节、末尾已有伪 tail 的失败形态，验证规范化后
`..` 在 4084 结束且 tail 成为独立记录。

## 验证

以下构建均通过：

```bash
cargo test --manifest-path os/vendor/another_ext4/Cargo.toml
make rv_check
make la_check
make kernel-la-ltp-glibc
```

使用 host `e2fsck -fy` 修复后的旧 LoongArch 初赛镜像副本，从两个目录仍带
`EXT4_INDEX_FL` 的状态启动 8 核 WaterOS。root-layout 完成过滤后目录标志正确变为普通
extent，运行后 `e2fsck -fn` 五阶段通过，返回 0。

修复后 QEMU 日志 SHA-256：
`3e48fc25064bfed753dab549e0e3e5a6f7380b30aad2bcfbddfc28002586a0a9`。
修复后 fsck 日志 SHA-256：
`1992a944c66d95a98bec32b60299f189627398daf2c52b5408f70fbfebad6a57`。

## RIO-10 阶段结果

同一组 8 核测试在 RISC-V64 和 LoongArch64 上结果一致：镜像中存在的 13 个 LTP 用例
全部通过，覆盖 `read`、`readv`、`pread`、`preadv`、pipe 和 eventfd fork 共享；
`open06`、`open09`、`read03`、`pipe03`、`pipe04` 在当前初赛镜像中不存在，runner 将其
记录为 missing。因此本轮证明双架构已有用例无回归，但不把 RIO-10 标记为全部完成。

RISC-V 日志 SHA-256：
`7fc03e98f046502c9eedaf8f9e0165c8ea9f2ed14959ec4106d2790b4f8f91fc`。
RISC-V fsck 日志 SHA-256：
`31646fa98a26218a2f52d8f59045c5d1bb3315024332c6ffda512765a4a50923`。
原始 RISC-V 初赛镜像 SHA-256：
`eed7f895f54a0a606d8bf05e2558650dd51f3b02b74b9703f6ad6fb1e8f03516`，测试前后未修改。

旧 LoongArch 初赛基线自身存在 superblock、inode 和目录损坏，必须修复副本后才能注入
runner；该轮不能用来证明原始 LA 镜像完整性，最终门禁仍需干净的 LoongArch 初赛镜像。
