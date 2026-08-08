# lazy file VMA 缺页查找改为二分

## 优化思路

`handle_lazy_page_fault()` 之前用 `Vec::iter().position()` 线性查找当前页所属的
lazy file VMA：

```rust
let Some(index) = self.lazy_file_vmas
                      .iter()
                      .position(|vma| vma.contains_page(page))
else {
    return Ok(false);
};
```

进程加载 ELF、动态库或 mmap 多个文件后，`lazy_file_vmas` 会包含多段 VMA。
BuildStorm 中频繁执行 ELF 装载和用户程序，缺页路径被线性扫描放大。

`register_lazy_file_vma()` 现在按 `start` 升序插入，`remove_lazy_file_vmas()` 和
`protect_lazy_file_vmas()` 继续维护有序、互不重叠集合。缺页查找改为二分定位第一个
`end > page` 的 VMA，再验证包含关系，复杂度从 O(n) 降到 O(log n)。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`

## 验证

- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V pre QEMU 60s smoke：rootfs RW 挂载成功，进入 busybox bringup，无 panic。

## 后续

下一步用 pc-hot A/B 对比 `handle_lazy_page_fault` / `handle_page_fault` 指令数，
并继续处理该函数中的逐页帧分配、零填充与 TLB flush 路径。
