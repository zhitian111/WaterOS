# ext4 延迟 inode 位图初始化修复验证

## 问题定位

LoongArch final 完整 BuildStorm 成功后，离线 `e2fsck -fn` 发现第 11 块组仍带
`INODE_UNINIT`，但 WaterOS 已在该组连续分配 744 个 inode：

```text
Group #11 has INODE_UNINIT set, but still contains 744 inodes.
Free inodes count wrong for group #11 (7448, counted=8192).
```

`another_ext4` 原先直接使用未初始化的 inode bitmap。按照 ext4 语义，该位图在
`INODE_UNINIT` 清除前没有有效的磁盘内容，因此运行时创建的文件可见，但标准工具会把
整组视为未使用，造成目录项、inode 和数据块引用在 fsck 中连锁失效。

## 修复

首次从带 `INODE_UNINIT` 的块组分配 inode 时：

1. 根据块组描述符的空闲 inode 数重建有效位；
2. 保留低编号的已分配或保留 inode，并将 bitmap block 的 padding bits 置 1；
3. 清除 `INODE_UNINIT`，再沿用原流程更新 bitmap checksum、块组 checksum、
   `itable_unused` 及 superblock 计数；
4. 若描述符空闲数超过组内 inode 数，返回 `EIO`，避免写回不可信元数据。

涉及文件：

- `os/vendor/another_ext4/src/ext4/alloc.rs`
- `os/vendor/another_ext4/src/ext4_defs/block_group.rs`

## 验证结果

静态与构建验证：

```bash
cargo test --manifest-path os/vendor/another_ext4/Cargo.toml
make la_check
make rv_check
make kernel-la-final
```

以上命令均通过。随后使用此前完整 BuildStorm 产生问题的 qcow2 overlay 合并 raw 镜像，
原位替换测试脚本以避免 host 工具提前初始化 bitmap，再由 8 核 LoongArch WaterOS 创建并
读取一个新文件。结果如下：

```text
INODE_UNINIT_PROBE start
inode-uninit-fixed
INODE_UNINIT_PROBE ok
Group 11: 7447 free inodes, 7447 unused inodes
e2fsck: Pass 1 ... Pass 5
E2FSCK_RC=0
```

第 11 组的 `INODE_UNINIT` 已清除；新增文件获得 inode 90857，原有 744 个 BuildStorm
inode 也通过目录、引用计数和块组汇总检查。fsck 仅提示部分 extent tree 可以收窄，
属于可选优化，不是一致性错误。

QEMU 串口日志 SHA-256：
`8ea8cf3060df84620db16b0cc432a791f218be6c704b279a1c92d560bd54eb2e`。
fsck 日志 SHA-256：
`87064fb2541bead1cdd4e8b11fafa40b80f2f065fbe43fe22ac25ed120987b69`。
