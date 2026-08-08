# 2026-08-08：LoongArch lazy VMA 有序插入修复

## 现象

LoongArch Final 复跑时 `BUILDSTORM_MINIBUILD fail`，rustc 报：

```text
failed to build archive ... failed to map object file: Invalid argument (os error 22)
```

## 根因

mmap 空闲区间搜索新增的冲突跳转使用二分定位 lazy VMA，但 LoongArch
`register_lazy_file_vma()` 之前是直接 `push`，没有保证按起始地址排序。RISC-V 一直是
有序插入，因此只有 LoongArch 会漏判重叠 lazy VMA，最终返回错误 mmap 地址。

## 修复

`impl-loongarch64/src/pagetable.rs` 改为与 RISC-V 一致：

```rust
let position = self.lazy_file_vmas.partition_point(|vma| vma.start.0 < start.0);
self.lazy_file_vmas.insert(position, ...);
```

## 验证

修复前 LoongArch Final：`BUILDSTORM_COMPILE ok=false`，约 331s 失败。

修复后 LoongArch Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1288.96 cores=8 bytes=1714568 arch=loongarch64
```

CAgent 10 项全部通过。

## 材料

```text
failed_log: /tmp/final-after-perf-la-20260808.log
  sha256: 0b97de45e5b8c498a77ef35c9b60489d5b87ad7cb176440ac51e9f0db9915f09
fixed_log: /tmp/final-after-perf-la-fixed-20260808.log
  sha256: 3d24b058f4ee9d464ac54cc43f8b2a8c6218fc2e69a8b7585230cd50477751fe
```

## 后续

- 双架构 Final 已恢复通过。
- 完整门禁仍需 iozone/LTP、`e2fsck -fn` 和掉电一致性。
