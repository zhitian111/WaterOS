# 任务 13：显式传播并启用 RISC-V lazy ELF

## 任务内容与目标

在 wateros-mm 聚合层增加明确的 `elf-lazy-map` feature 传播，并只在 RISC-V final 性能候选
中启用。验证 page fault、fork/exec、动态解释器、共享只读页和 BuildStorm 收益；不得依赖
实现 crate 被 `default-features=false` 屏蔽的默认值。

## 实施方案

1. `wateros-mm` feature 同时定义到双实现的传播关系，但本提交仅在 RISC-V profile 启用。
2. 保存 feature tree，证明 candidate 编译 lazy 分支、baseline 编译 eager 分支。
3. 复用任务 12 ExecFile，记录初始驻留页、fault、共享页 hit、RSS/frame 和 exec wall time。
4. 覆盖重叠 PT_LOAD、BSS zero、R/X/W 权限、PIE、PT_INTERP、fork 后 fault、unlink 后 fault。
5. 若完整 A/B 无稳定收益或出现 fault/权限回归，关闭 profile feature并保留失败简报。

## 涉及文件

- `os/components/wateros-mm/Cargo.toml`
- `os/Cargo.toml` 的 RISC-V platform profile
- 必要时 `impl-sv39/src/{kernel_elf,pagetable,user_heap_mmap}.rs`
- feature/build 文档：`os/README.md`、`docs/tools/makefile.md` 等触发项

## CodeGraph 查询

```bash
codegraph explore "elf-lazy-map map_segment_from_path_lazy ElfPathSegmentLoader handle_page_fault"
codegraph impact "DemandPageLoader"
codegraph callers "load_shared_page"
```

## 验收方式

```bash
cd os
cargo tree -e features | rg "elf-lazy-map"
make rv_check && make la_check && make kernel-rv-final
# 运行 ELF/exec/fork/page-fault 定向回归
cd .. && git diff --check
```

使用任务 00 的 QEMU 9.2.1 `-snapshot` runner 做交错 eager/lazy A/B，至少两轮候选夹一轮
baseline；候选中位数需有可复现收益且所有 BuildStorm marker 完整。记录共享页命中与内存量。

## Commit 与简报

提交建议：`[perf] RISC-V final 启用 lazy ELF`。新增 `history/13-brief.md`，附 feature tree、
完整日志、A/B 表和保留/回退结论。
