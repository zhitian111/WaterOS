# Task 01：把 VMA 类型与基础方法抽到 `mm-impl/common`

## 任务目标

消除 Sv39 与 LoongArch64 中重复的 VMA 结构定义，把类型、contains/overlaps、
duplicate 等基础方法放到共享层，两个 impl 通过引用/泛型复用。本任务不改变行为。

## 实施方案

1. 在 `mm-impl/common` 增加 VMA 共享模块：

   ```text
   mm-impl/common/src/vma/
   ├── mod.rs
   ├── lazy_file.rs
   ├── shared_anon.rs
   ├── shared_file.rs
   └── device.rs
   ```

2. 保持现有字段语义：

   - `LazyFileVma { start,end,perm,file_offset,file_size,loader }`
   - `SharedAnonVma { start,end }`
   - `SharedFileVma { start,end,file_offset,loader }`
   - `DeviceVma { start,end,phys_start,perm,lease }`

3. 基础方法优先移到共享层，`duplicate` 可先保留 trait/泛型适配。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/lib.rs`
- 新增 `os/components/wateros-mm/mm-impl/common/src/vma/**`
- 两套 `pagetable.rs` 的 struct 定义改为引用/重新导出

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "LazyFileVma SharedFileVma DeviceVma"
codegraph impact "LazyFileVma"
```

## 验收方式

```bash
cd /tmp/wateros-vma-unified/os
make rv_check
make la_check
git diff --check
```

- 双架构编译通过；
- `rg -n "struct LazyFileVma|struct SharedAnonVma|struct SharedFileVma|struct DeviceVma" components/wateros-mm` 确认定义不再重复。

## 完成后

新增 `history/01-brief.md`。
