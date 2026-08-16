# Task 06 简报：lazy 缺页入口收口到 `mm-impl/common`

## 完成情况

完成文件/匿名 lazy VMA 缺页路径的架构无关化：

- 在 `mm-impl/common` 新增 `fault.rs`，提供
  `handle_lazy_file_fault` 与内部 `LazyVmaAccess` trait；
- `handle_lazy_file_fault` 负责 VMA 查找、权限检查、只读页优先走
  `VmaBacking::load_shared_page`（现有只读 page cache），私有/可写页分配新帧并走
  `VmaBacking::load_page`；
- Sv39 / LoongArch64 的 `handle_lazy_page_fault` 现在只保留架构相关 TLB
  刷新，具体缺页逻辑都调用统一入口；
- 删除 Sv39 中不再使用的 `lazy_file_vma_index` 私有方法。

## 改动文件

- `os/components/wateros-mm/mm-impl/common/src/lib.rs`
- `os/components/wateros-mm/mm-impl/common/src/fault.rs`（新增）
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`

## 验收

```text
make rv_check          PASS
make la_check          PASS
make kernel-rv-final   PASS
make kernel-la-final   PASS
git diff --check       PASS
```

## 未验证项

- 运行时完整 BuildStorm（RV 单核 / RV 8 核 / LA 单核 / LA 12 核）尚未执行；
- 性能回归尚未执行。

原因是当前系统中有其它 QEMU 进程在运行，按约定性能测试会等它退出后再进行；
本任务先完成静态实现和双架构构建校验。
