# 任务 12：用 ExecFile 统一 exec 稳定句柄和 ELF 读取

## 任务内容与目标

引入 `ExecFile`，让 exec 预检的 256 字节前缀、ELF header、program headers、PT_LOAD lazy
loader 和动态解释器读取共享稳定 VFS handle，消除重复路径解析与每个 PT_LOAD 单独 open。
本提交保持 eager/lazy feature 状态不变。

## 实施方案

1. `ExecFile` 持有 resolved executable path、stable handle、metadata/content identity，并提供
   `read_exact_at`/prefix 方法。
2. shebang 递归每层各自创建一个 ExecFile；最终 argv/executable_path 语义不变。
3. `from_elf_path` 内部改为接收/构造 ExecFile；所有 segment loader clone 同一 handle/identity。
4. PT_INTERP 创建独立稳定 ExecFile，并在 auxv 中保持原路径语义。
5. 双架构共享能共享的契约，架构 ELF 校验保持各自模块；unlink/rename 后 fault 仍读原 inode。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common` 或 MM facade 的 exec file 模块
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/{kernel_executable,kernel_elf}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs`
- VFS stable handle API 与测试

## CodeGraph 查询

```bash
codegraph explore "load_program_from_path from_elf_path ElfPathSegmentLoader read_path_exact"
codegraph impact "from_elf_path"
codegraph callers "ElfPathSegmentLoader::new"
```

## 验收方式

```bash
cd os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# ELF、shebang、PT_INTERP、unlink-after-exec、并发 exec 定向回归
cd .. && git diff --check
```

同一 ELF 只解析/open 一次，所有 PT_LOAD 共享 handle；eager loader 行为和 BuildStorm 结果不变。
任务 00 runner A/B 用于证明该结构改动本身无回退。

## Commit 与简报

提交建议：`[perf] exec 使用统一稳定文件句柄`。新增 `history/12-brief.md`。
