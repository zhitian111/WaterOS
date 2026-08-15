# Task 01 简报：VMA 类型抽到 `mm-impl/common`

## 完成情况

完成。`LazyFileVma`、`SharedAnonVma`、`SharedFileVma`、`DeviceVma` 及其基础方法
已从 Sv39 / LoongArch64 两套 `pagetable.rs` 移到共享层 `mm-impl/common/src/vma.rs`。
两个 impl 通过 `pub(crate) use impl_common::...` 重新导出，保持现有调用点不变。

## 改动文件

- `os/components/wateros-mm/mm-impl/common/src/vma.rs`（新增）
- `os/components/wateros-mm/mm-impl/common/src/lib.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`

## 验收命令与结果

```text
make rv_check          PASS（仅有既有 warning）
make la_check          PASS（仅有既有 warning）
make kernel-rv-final   PASS
make kernel-la-final   PASS
git diff --check       PASS
```

## 未验证项

- 尚未做 QEMU 运行时回归；本任务只做结构搬移，不改变运行语义。
- Task 02 将继续把修改操作收口到统一 VMA 注册表。
