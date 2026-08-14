# 任务 02：新增 host 侧目录块边界回归

## 状态

待实施。

## 目标

把任务 01 的目录块 tail 边界变成一个可在宿主机上反复执行的端到端回归，
证明修复后的文件系统在“目录块填满到 12 字节 tail 边界”时不会产生乱码目录项，
并且宿主 `e2fsck -fn` 通过。

## 背景

任务 01 的单元测试覆盖了 `DirBlock` 内存行为，但没有覆盖完整 ext4 落盘后
`e2fsck` 的校验。需要一个轻量 host 程序：

1. 创建 512MB / 4K block 的 ext4 镜像；
2. 用一个目录写入足够多的 12 字节目录项，逼出 tail 边界；
3. 追加一个 3 字符子目录 `vim`；
4. `listdir` 校验条目完整、无非法 UTF-8 名；
5. `flush_all` 后由外部 `e2fsck -fn` 校验。

## 涉及文件

建议新增独立 crate，避免改动现有 `ext4_test` 的 `simple_logger` 依赖：

- `os/vendor/another_ext4/ext4_regression/Cargo.toml`
- `os/vendor/another_ext4/ext4_regression/src/main.rs`
- `os/vendor/another_ext4/ext4_regression/src/block_file.rs`

如团队更希望复用 `ext4_test`，则本任务可改为改造
`os/vendor/another_ext4/ext4_test`，但必须保证 `--offline` 可构建。

## 实施方案

1. 新建 `ext4_regression` crate，仅依赖：

   ```toml
   another_ext4 = { path = "..", features = ["block_cache"] }
   ```

2. `block_file.rs` 实现 `another_ext4::BlockDevice`（4K block 随机读写文件）。
3. `main.rs` 流程：

   - `dd if=/dev/zero of=<tmp>.img bs=1M count=512`
   - `mkfs.ext4 -b 4096 <tmp>.img`
   - `Ext4::load` 后 `mkdir d`；
   - 创建 360 个 4 字符文件名（`required_size == 12`）；
   - `mkdir d/vim`；
   - `listdir(d)` 检查：
     - 条目数正确；
     - 所有 `DirEntry::name()` 经 `std::str::from_utf8` 校验合法；
     - `vim` 存在；
   - `ext4.flush_all()`；
   - 退出码反映结果，外部脚本再调用 `e2fsck -fn`。

4. 提交该回归工具，提交信息：

   ```text
   [test] another_ext4 新增目录块 tail 边界 host 回归
   ```

## CodeGraph 查询命令

```sh
codegraph explore "BlockDevice Ext4 load mkdir listdir"
codegraph node "another_ext4/src/ext4_defs/block.rs"
codegraph callers "DirBlock::insert"
```

索引未覆盖 vendor 时回退：

```sh
rg -n "trait BlockDevice|pub fn mkdir|pub fn listdir" os/vendor/another_ext4/src
```

## 验收命令

```sh
cd /tmp/WaterOS_ext4_dir_tail_fix
cargo run --offline --manifest-path os/vendor/another_ext4/ext4_regression/Cargo.toml
e2fsck -fn /tmp/<regression-image>.img
git diff --check
```

验收标准：

- host 回归程序打印 `OK` 或等价的成功标记；
- `e2fsck -fn` 五阶段通过，无 `illegal characters`、`fails checksum`；
- `git diff --check` 干净。

## 完成后简报

写 `history/02-add-host-dir-tail-regression-brief.md`，记录新增 crate 路径、
运行输出摘要和 e2fsck 结果。
