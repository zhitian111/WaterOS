# another_ext4 索引目录写入修复记录

## 问题

`another_ext4` 不实现 HTree 更新，却会把 indexed 目录的 dx root 当成普通目录项空闲区
写入，导致压力测试后目录索引损坏。首次清除索引标志后还发现 dx root 块没有普通目录
checksum tail，离线检查会报告目录块 checksum 错误。

## 修复

- 在索引目录首次增删前清除 inode 的 `EXT4_INDEX_FL`。
- 将 dx root 块转换为线性目录块：收缩覆盖块尾的目录项，建立 12 字节 checksum tail，
  重新计算块 checksum，并持久化 inode checksum。
- 后续继续使用项目现有的全块线性扫描和目录项增删路径。

## 验收

- 在干净基线的 indexed 目录 inode 22501 中创建并删除文件，输出
  `DIR_TAIL_TEST_DONE`。
- inode flags 从 `0x81000` 变为 `0x80000`。
- 覆盖层 `qemu-img check` 与 `e2fsck -fn` 均无错误。
- QEMU 日志 `/tmp/wateros-dir-tail-probe-20260803.log`，SHA-256：
  `4281f47ff037a16e07b8fc97c9ad657d55e710f95054f83823bc5ae69a43f0c2`。
