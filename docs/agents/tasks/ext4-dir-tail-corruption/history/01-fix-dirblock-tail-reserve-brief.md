# 任务 01 简报：修复 DirBlock 目录块校验尾被覆盖

## 完成情况

- 已修复 `another_ext4` 的 `DirBlock` 将目录项写入 12 字节 `DirEntryTail` 的问题。
- 未在 vendor 内新增 `#[test]`（main 分支不接受）；340/341 边界回归由任务 02
  的 host 回归与任务 03 的 QEMU 回归覆盖。
- 提交：`9f4ddd9e [fix] another_ext4 目录项禁止占用 checksum tail`

## 修改文件

- `os/vendor/another_ext4/src/ext4_defs/dir.rs`

## 验收结果

```text
cargo test --offline --manifest-path os/vendor/another_ext4/Cargo.toml --features block_cache
3 passed; 0 failed

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
