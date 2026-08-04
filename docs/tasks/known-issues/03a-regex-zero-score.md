# K-03A：libc-bench regex_search 0 分

## 当前进展

**2026-08-04 已完成。** RISC-V64/LoongArch64 的 glibc/musl 四种组合均使用
原始 `libc-bench` 运行到 regex 末尾并退出 0，两个历史 0 分 search 项均产生非零
耗时。结果见 [`results/k03a-final-20260804.md`](results/k03a-final-20260804.md)。

## 任务目标

复验四配置 regex_search 历史 0 分，并区分结果解析、超时、栈溢出、signal 和 MM
错误。只修最小根因。

## 执行前必读

- `docs/tasks/known-issues/03-functional-zero-scores.md`
- `docs/prompts/general.md`
- `docs/prompts/debug_workflow.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/tasks/analyze_kernel_log.md`

## 已知信息与代码证据

历史上 regex compile 正常，仅两个 search 表达式为 0。当前 trap 会记录用户 fault 的
PC、VA 和 task，因此最小复现必须保留这些字段：

```text
[trap] killing user task ... pc=<pc> fault_addr=<va> task_id=<id>
```

不能在没有 trap/exit 证据时扩大用户栈或修改 regex benchmark。

## 涉及文件

- `os/src/trap_handler.rs`
- `os/components/wateros-mm/`
- `os/components/wateros-ipc/ipc-signal/`
- `os/components/wateros-syscall/`
- `test_case/` 中 libcbench 脚本和二进制

## 任务内容

1. 单独运行两个表达式，保存退出码、耗时、signal、PC/VA、stack mapping 和最后 syscall。
2. 在 Linux 及四个 WaterOS 配置对照，确认是内核失败还是 harness 解析。
3. 最小化为单个 C 程序；按首个根因修改 MM/signal/syscall 所属层。
4. 修复后运行其它 regex、深栈、mmap/mprotect 和 signal frame 回归。

## 如何验收

- [x] 可运行配置中 regex_search 均 pass 或得到有效 score。
- [x] 根因有最小复现和 Linux 对照，不靠增加 timeout 掩盖。
- [x] 无用户栈、signal、mmap 与其它 libcbench 回归。
- [x] `make rv_check && make la_check` 通过。

交付 `docs/tasks/known-issues/results/k03a-YYYYMMDD.md`。
