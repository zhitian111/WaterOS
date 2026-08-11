# ext4 延迟块位图初始化修复验证

## 问题定位

LoongArch final 的 BuildStorm 写入负载后，离线执行 `e2fsck -fn` 报告第 38
块组仍带 `BLOCK_UNINIT`，但已经分配了 525 个块：

```text
Block bitmap differences:  +(1245184--1245708)
Free blocks count wrong for group #38 (32768, counted=32243)
```

测试前的主办方镜像检查正常，因此损坏来自运行时分配。`another_ext4` 原先直接在
未初始化的块位图中寻找空闲位，没有实现 ext4 `BLOCK_UNINIT` 语义，首次分配后写回
了不完整的位图和校验和。

## 修复

首次从带 `BLOCK_UNINIT` 的块组分配时：

1. 根据块组描述符的空闲块计数重建位图；
2. 保留块位图、inode 位图、inode table 及低地址 sparse-super/GDT 开销；
3. 清除 `BLOCK_UNINIT`，随后沿用原有流程更新位图校验和、描述符校验和及计数。

涉及文件：

- `os/vendor/another_ext4/src/ext4/alloc.rs`
- `os/vendor/another_ext4/src/ext4_defs/bitmap.rs`
- `os/vendor/another_ext4/src/ext4_defs/block_group.rs`

## 验证结果

执行：

```bash
cargo test --manifest-path vendor/another_ext4/Cargo.toml
make la_check
make kernel-la-final
WOS_SMP=8 make la_final_run
e2fsck -fn /tmp/wateros-la-final-buildstorm-blockinit.img
```

结果：

- `another_ext4` 测试和 `make la_check` 通过；
- CAgent 脚本退出码为 0，BuildStorm toolchain/minibuild 通过；
- 同等 BuildStorm 写入后，`e2fsck` 五阶段检查通过，不再出现位图、空闲计数或
  `BLOCK_UNINIT` 错误；
- 仅保留 e2fsck 的 extent tree 可收窄优化提示，不属于一致性错误。

修复后串口日志 SHA-256：
`83a302a55e2acf5e44a04fefa8e49192df41e024c78fa749013f9d936a03b3c6`。
修复后 fsck 日志 SHA-256：
`2325b84a4a64f42c3bfea67754cb9f88e619aee6be24c2e914981bd7b0570a75`。

## 独立剩余问题

BuildStorm 随后仍报告相对目标路径
`scripts/targets/std/pie/loongarch64-unknown-linux-musl.json` 不存在。该文件在测试前后
均可从镜像离线读取，因此不是本次块位图损坏导致；应继续检查子进程工作目录继承和
相对路径解析。
