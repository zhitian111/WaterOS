# LoongArch 决赛镜像启动与 CAgent 验证

## 问题与修复

LoongArch final 最初无法进入用户测试，依次暴露四个独立问题：

1. SMP 把编译期 `MAX_CPUS=32` 当作 QEMU 实际 CPU 数，`-smp 8` 时等待不存在的
   AP。现从 QEMU virt 固定地址 `0x100000` 的 DTB `/cpus` 解析 configured mask。
2. ELF 装载器没有跟随程序和 `PT_INTERP` 的符号链接，导致 glibc 动态链接器返回
   `RootVolume(NotAFile)`。现统一通过 VFS `FinalSymlink::Follow` 解析。
3. glibc 动态链接器在 `0x700177d4` 执行 `vld`，触发 `ECODE=16`。现启用 LSX，
   并在用户 trap 帧保存/恢复 32 个 128-bit 向量寄存器和 `FCSR0`，覆盖抢占和迁核。
4. LoongArch MM 错把 PLV0 DMW 物理恒等窗口当作用户 VA 保留区，拒绝位于
   `0x120000000` 静态程序之后的 `brk`。移除该虚假冲突检查后，仍保留用户上限、
   栈和 VMA 冲突校验。

## 验证结果

执行：

```bash
make la_check
make kernel-la-final
WOS_QEMU_SNAPSHOT=1 WOS_SMP=8 make la_final_run
```

结果：

- DTB configured mask 为 `0xff`，8 个 CPU 全部 online；
- `cagent-glibc` 10 项全部 `pass`，脚本退出码为 0；
- BuildStorm 输出 `BUILDSTORM_TOOLCHAIN ok`，短测窗口结束时仍在继续；
- 没有再次出现 `NotAFile`、`ECODE=16` 或高地址 `brk` 写页错误；
- `make la_check` 和 final release 构建通过。

测试日志为 `/tmp/wateros-la-final-context-20260803.log`，SHA-256：
`6881e95d4a2b891b3e75e83d7905de778e4fff7ec2c9f86c769cdf47b40c979d`。
内核 SHA-256：`7b9b59bed09c1180a0938204ee27b717b6e14e2cc99a11c25ed5c646321690a7`。
镜像 SHA-256 仍为
`cf8660bdc216d3dd6c82f4b50cdc4271d1be6dc49eb647ccbb9a0f24f36ad245`。

## 剩余验证

本记录只宣告 LoongArch final 的 SMP 启动和 CAgent 阶段通过。BuildStorm 完整编译、
产物检查及测试后 ext4 一致性需要单独长测并另写结果记录。
