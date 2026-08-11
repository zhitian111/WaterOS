# K-23 最佳候选构建入口收口（2026-08-05）

## 问题

`os/Makefile` 的 `make all` 仍通过 `kernel-rv`、`kernel-la` 构建初赛配置，与当前已经
完成双架构 CAgent 和 BuildStorm 验证的 Final 候选不一致。根 Cargo manifest 的默认
feature 也没有选择 `pre` 或 `final_online`，使用 RISC-V 交叉目标执行 Cargo 默认
feature 构建会触发阶段选择编译错误。

## 修改

- Cargo 默认配置选择 RISC-V64、`final_online` 和已验证的 TLSF heap。
- `make all` 构建 RISC-V64、LoongArch64 的 release Final 内核。
- 构建结果同时保留为 `kernel-*-final`，并复制为在线提交约定的 `kernel-rv`、
  `kernel-la`。
- Final 候选不启用 `bringup-stats`、`stall-debug`、`dashboard-debug`、`gdb-debug` 或
  故障注入；显式的 Pre、调试和回退 allocator 目标保持可用。

## 验证

```text
make all
cargo check --release --target riscv64gc-unknown-none-elf
make rv_check
make la_check
```

验收还检查四个交付 ELF 存在，通用文件与对应 Final 文件 SHA-256 完全一致，并核对
Cargo feature tree 中使用 TLSF、block cache 且不含诊断 feature。RISC-V Final 当前使用
已经验证的 lazy ELF；LoongArch Final 保持完整 BuildStorm 已验证的 eager ELF，不能在
没有独立 A/B 和正确性回归的情况下把其可选 `elf-lazy-map` 当作已知最优配置。

```text
kernel-rv  = f28d45ab8e3591a90f2db2c4a34409e02f578acd776dd91ab4098b08a87c1c57
kernel-la  = e685476ee5d7dc799481d7eae104609fba0b58d4a31bb5e14ad8d2239ceb7149
```

使用上述通用文件名、8 vCPU、4 GiB 和 QEMU snapshot 做 75 秒双架构并行 smoke：两边
CAgent 均输出十条 pass 且命令退出码为 0，随后进入 BuildStorm；RISC-V 通过 toolchain
和 minibuild 后开始正式编译，LoongArch 通过 toolchain。测试按预期由宿主 timeout
终止，未用于替代已有完整 BuildStorm 结果。

此前双架构 Final 完整运行证据见 `k22-dual-arch-final-buildstorm-20260805.md`；本任务不
修改运行时代码或 task 模块架构。
