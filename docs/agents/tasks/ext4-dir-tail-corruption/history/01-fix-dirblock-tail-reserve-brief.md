# 任务 01 简报：修复 DirBlock 目录块校验尾被覆盖

## 完成情况

- 已修复 `another_ext4` 的 `DirBlock` 将目录项写入 12 字节 `DirEntryTail` 的问题。
- 已新增 `insert_reserves_checksum_tail` 单元测试覆盖 340/341 边界。
- 提交：`096df78a [fix] another_ext4 目录项禁止占用 checksum tail`

## 修改文件

- `os/vendor/another_ext4/src/ext4_defs/dir.rs`

## 验收结果

```text
cargo test --offline --manifest-path os/vendor/another_ext4/Cargo.toml --features block_cache
4 passed; 0 failed

cargo check --offline --manifest-path os/vendor/another_ext4/Cargo.toml --features block_cache
Finished

cargo check --offline --manifest-path os/components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
Finished

git diff --check
（无输出，通过）
```

## 未验证 / 剩余风险

- 尚未运行 QEMU `apt/dpkg` 端到端回归；该验证由任务 03 承接。
- host 侧完整文件系统落盘 `e2fsck -fn` 回归由任务 02 承接。
