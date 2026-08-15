# Task 05 简报：loader 字段抽成 `VmaBacking`

## 完成情况

完成第一版解耦：`LazyFileVma` 和 `SharedFileVma` 不再直接持有
`Box<dyn DemandPageLoader>` 字段，改为持有 `VmaBacking`。

`VmaBacking` 当前有两个语义变体：

- `Anonymous`：零页/COW；
- `File { loader }`：暂保留 loader，Task 06 再替换为统一 page cache backing。

## 改动文件

- `mm-impl/common/src/vma.rs`
- 两套 `pagetable.rs`
- 两套 `kernel_elf.rs`
- 两套 `user_heap_mmap.rs`
- 两套 `impl-*/src/lib.rs`

## 验收

```text
make rv_check          PASS
make la_check          PASS
make kernel-rv-final   PASS
make kernel-la-final   PASS
git diff --check       PASS
```

## 未验证项

- QEMU 完整 BuildStorm 回归延后到 Task 07 统一执行。
