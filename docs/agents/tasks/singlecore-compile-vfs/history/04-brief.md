# Task 04：exec ELF 前缀复用实验简报

## 实验内容

提交 `23b5535b` 尝试让 `load_program_from_path` 在确认 ELF 后，把已经完成路径解析并读取的
文件前缀直接交给 ELF loader，避免 `from_elf_path` 再次解析同一路径和重读 64 字节 ELF
header。两架构实现保持一致，没有修改页表、COW、解释器映射或进程生命周期语义。

## 静态验证

以下四组检查通过：

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
make rv_check
make la_check
```

RV slab 与 LA TLSF 的 final 内核也构建通过。

## 一轮运行结果

先在 LA + TLSF 上按 16 GiB、12 CPU、`-snapshot` 口径筛选：

```text
对照:  BUILDSTORM_RESULT elapsed_s=538.04 status=OK run=OK
候选:  BUILDSTORM_RESULT elapsed_s=576.61 status=OK run=OK
```

候选慢 38.57 秒（约 7.17%）。功能虽然通过，但明显违反单轮筛选门槛，因此没有继续浪费
一轮 RV；代码已回退。该绝对回退可能包含宿主噪声，不能据此证明重复读取本身有害，但它
足以说明这项微优化没有可辨识的稳定收益，不进入最终组合。

