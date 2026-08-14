# 任务 02 简报：新增 host 侧目录块边界回归

## 完成情况

- 新增 `os/vendor/another_ext4/ext4_regression` crate，用于在宿主机上复现
  目录块 12 字节 `DirEntryTail` 边界。
- 回归程序创建 512MB / 4K 镜像，向单个目录写入 360 个 4 字符文件，再创建
  3 字符子目录 `vim`，校验 `listdir` 条目完整、无非法 UTF-8 名。
- 提交：`6749a6c0 [test] another_ext4 新增目录块 tail 边界 host 回归`

## 修改文件

- `os/vendor/another_ext4/ext4_regression/Cargo.toml`
- `os/vendor/another_ext4/ext4_regression/src/block_file.rs`
- `os/vendor/another_ext4/ext4_regression/src/main.rs`

## 验收结果

```text
cargo run --offline --manifest-path os/vendor/another_ext4/ext4_regression/Cargo.toml
OK /tmp/ext4-dir-tail-regression.img

e2fsck -fn /tmp/ext4-dir-tail-regression.img
Pass 1..5 全部通过，无错误输出

git diff --check
（无输出，通过）
```

## 未验证 / 剩余风险

- 本任务是 host 侧完整文件系统回归；真实 QEMU `apt/dpkg` 端到端验证仍由
  任务 03 承接。
