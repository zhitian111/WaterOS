# K-03：功能性 0 分项复现与修复

## 任务目标

重新验证旧评分中的 libc-bench regex、musl-rv Pagefaults 和 busybox
kill/mv/rmdir 0 分项。每个分项先得到最小复现和真实 errno/trap，再独立修复；不得把
功能失败当作性能问题。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/tasks/analyze_kernel_log.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/todo/perf-baseline-gap-report.md`

## 已知信息与代码证据

旧基线记录：

| 分项 | 历史现象 | 当前可能已变化的原因 |
|---|---|---|
| G4 | regex_search 两项在四配置均为 0 | MM、signal、栈和 lazy map 已有后续改动 |
| G5 | 仅 musl-rv Pagefaults 为 0 | Sv39 lazy fault/COW 路径已有后续改动 |
| G9 | kill、mv、rmdir 在四配置为 0 | signal、rename、目录增长和 root layout 已修过 |

当前 root layout 特意不再创建名为 `test` 的 busybox 链接，避免阻挡
`mv test_dir test`：

```rust
/// `test` 不链到 `/glibc`/`/musl` 根：赛题 busybox 用例会
/// `mv test_dir test` 建目录。
```

因此不能根据旧 score 直接再次修改 root layout。

## 涉及文件

- `os/src/user_bringup_busybox.rs`
- `os/src/user_bringup_root_layout.rs`
- `os/src/trap_handler.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/{signal,kill_target}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/renameat2.rs`
- `os/components/wateros-vfs/`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/{pagetable,user_heap_mmap,kernel_elf}.rs`
- `test_case/` 中对应 busybox/libcbench/lmbench 脚本
- `docs/todo/perf-baseline-gap-report.md`

## 可并行任务

- [`K-03A：regex_search`](./03a-regex-zero-score.md)
- [`K-03B：musl-rv Pagefaults`](./03b-musl-rv-pagefault.md)
- [`K-03C：busybox kill/mv/rmdir`](./03c-busybox-kill-mv-rmdir.md)

三个任务使用不同主要模块，可在独立 worktree 并行；最终统一更新 score 基线。

## 任务内容

将三个分项拆成三个独立工作目录或提交，可并行执行：

1. **G4 regex**：只运行失败表达式，记录退出码、signal、fault VA、PC、用户栈余量和
   最后 syscall。先区分 benchmark 输出解析、超时、用户栈溢出和 MM/signal 错误。
2. **G5 Pagefaults**：用相同二进制和参数对比 glibc-rv、musl-rv；记录 ELF 布局、
   fault access 类型、VMA、COW/lazy 分支和 errno。不要从 libc 名称硬编码内核行为。
3. **G9 busybox**：逐条执行 `kill`、`mv test_dir test`、`rmdir test`，检查命令自身
   返回值及操作后状态。`mv` 失败可能导致后续 `rmdir` 连锁失败，必须分开建干净
   目录。

最小测试应同时检查返回值和副作用：

```sh
mkdir test_dir
mv test_dir test
test -d test
rmdir test
test ! -e test
```

只在当前 main 可复现时修改代码。若已经通过，更新基线报告并关闭该分项，不制造空
重构。每个修复只进入所属 API/impl：signal 语义在 syscall/task/ipc，目录操作在
VFS/FS，缺页在 MM。

## 如何验收

- [ ] 四种 libc/arch 组合中可运行的组合均有最新结果，不再引用旧 score 代替复验。
- [ ] 每个历史 0 分项变为 pass 或有效 score；不可运行组合有精确阻断。
- [ ] errno、文件系统副作用和 signal 结果与 Linux 对照一致。
- [ ] `make rv_check && make la_check` 通过。
- [ ] G4 修复后运行 signal/mmap/stack 相关回归。
- [ ] G5 修复后运行 COW、lazy mmap、fork 和 exec 回归。
- [ ] G9 修复后运行 busybox 及 rename/unlink/rmdir LTP 子集，并执行 `e2fsck -fn`。

每个分项独立记录到 `docs/tasks/known-issues/results/k03-<g4|g5|g9>-YYYYMMDD.md`。
