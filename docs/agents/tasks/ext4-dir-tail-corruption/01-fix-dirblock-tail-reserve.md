# 任务 01：修复 DirBlock 目录块校验尾被覆盖

## 状态

待实施（准备阶段已完成，代码草案已拷入本工作树但未提交）。

## 目标

修复 `another_ext4` 的 `DirBlock` 在目录块末尾 12 字节 `DirEntryTail` 区域
写入普通目录项，随后 `set_checksum` 又把该目录项 name 字段覆盖为 CRC，
最终产生乱码目录项和无效目录 checksum。

这是 `neovim-runtime` 解包失败的根因，不是 extent 树 split 的问题。

## 根因

`os/vendor/another_ext4/src/ext4_defs/dir.rs` 中 `DirBlock::insert` 的遍历上限是
`BLOCK_SIZE`，会把目录块末尾的 `DirEntryTail` 当作一个 `rec_len == 12` 的空闲项。
当目录块剩余空间只剩 tail，而新目录项 `required_size == 12`（例如 3 字符子目录名
`vim`）时，`insert` 会把目录项写进 tail；随后 `set_checksum` 在同一 offset 写回
`DirEntryTail`，覆盖目录项 name，且 tail 的保留字段被目录项字节污染。

## 任务内容

1. 在 `DirBlock` 增加 `TAIL_OFFSET` 常量。
2. `get` / `list` / `insert` / `remove` 的扫描范围限制在 `TAIL_OFFSET` 之前，
   永不把 tail 当作目录项空间。
3. `insert` 在 tail 之前无可用空间时返回 `false`，让 `dir_add_entry` 走
   “追加新目录块”的正常路径。
4. `set_checksum` / `init` / `ensure_tail` 统一使用 `TAIL_OFFSET`。
5. 新增单元测试 `insert_reserves_checksum_tail`，覆盖“第 341 个 12 字节目录项
   不得进入 tail”的边界。

## 涉及文件

- `os/vendor/another_ext4/src/ext4_defs/dir.rs`

本任务只改 vendor 的目录块布局实现，不触碰 extent 逻辑、不触碰 `api-v0`。

## 实施方案

1. 确认工作树干净（除本任务文件外无其它改动）：

   ```sh
   cd /tmp/WaterOS_ext4_dir_tail_fix
   git status --short
   ```

2. 将代码草案与下述意图逐项核对后提交。
3. 核心改动示意：

   ```rust
   const TAIL_OFFSET: usize = BLOCK_SIZE - size_of::<DirEntryTail>();

   // get/list/insert/remove 使用 `while offset < Self::TAIL_OFFSET`
   // insert 中额外防护：
   if rec_len == 0 || offset + rec_len > Self::TAIL_OFFSET {
       break;
   }
   ```

4. 新增单元测试：

   ```rust
   #[test]
   fn insert_reserves_checksum_tail() {
       let mut dir = DirBlock::new(Block::default());
       dir.init();
       for inode in 1..=340 {
           assert!(dir.insert("f000", inode, FileType::RegularFile));
       }
       assert!(!dir.insert("f000", 341, FileType::RegularFile));
       let tail_offset = BLOCK_SIZE - size_of::<DirEntryTail>();
       let tail: DirEntryTail = dir.block().read_offset_as(tail_offset);
       assert_eq!(tail.rec_len as usize, size_of::<DirEntryTail>());
       assert_eq!(tail.reserved_ft, 0xDE);
   }
   ```

5. 提交信息格式：

   ```text
   [fix] another_ext4 目录项禁止占用 checksum tail
   ```

## CodeGraph 查询命令

若本工作树的 CodeGraph 已索引 vendor：

```sh
codegraph explore "DirBlock insert set_checksum"
codegraph node "DirBlock"
codegraph callers "DirBlock::insert"
codegraph impact "DirBlock::insert"
```

若 vendor 未进入索引，回退：

```sh
rg -n "while offset < BLOCK_SIZE|set_checksum|DirEntryTail|fn insert" \
  os/vendor/another_ext4/src/ext4_defs/dir.rs
```

## 验收命令

从工作树根目录执行：

```sh
cd /tmp/WaterOS_ext4_dir_tail_fix
cargo test --offline --manifest-path os/vendor/another_ext4/Cargo.toml --features block_cache
cargo check --offline --manifest-path os/vendor/another_ext4/Cargo.toml --features block_cache
cargo check --offline --manifest-path os/components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
git diff --check
```

验收标准：

- 上述命令全部通过；
- `cargo test` 中 `insert_reserves_checksum_tail` 和原有目录块测试均通过；
- `git diff --check` 无空白错误。

## 完成后简报

写 `history/01-fix-dirblock-tail-reserve-brief.md`，说明提交 hash、验证结果和
是否已把 host 复现纳入后续任务。
