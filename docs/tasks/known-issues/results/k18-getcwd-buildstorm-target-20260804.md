# getcwd ABI 与 LoongArch BuildStorm 相对目标路径修复

## 问题定位

LoongArch final 的 Cargo 可以通过 `stat` 读取
`scripts/targets/std/pie/loongarch64-unknown-linux-musl.json`，但使用同一相对路径时报告：

```text
error: target path `scripts/targets/std/pie/loongarch64-unknown-linux-musl.json` is not a valid file
Caused by: No such file or directory (os error 2)
```

绝对路径可以进入编译，相对路径失败。进一步检查发现 `sys_getcwd()` 成功时返回了
用户缓冲区地址；Linux syscall ABI 要求返回包含结尾 NUL 的字节数，缓冲区地址是 libc
包装函数的返回值。glibc 因此把 Cargo 的相对路径规范化判为失败。

## 修复

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/cwd.rs` 在成功复制 cwd
后返回 `written`，不再返回 `buf_ptr`。修改不涉及 task 数据结构、cwd 继承规则或调度器。

## 验证结果

1. `make la_check` 和 `make kernel-la-final` 通过。
2. 修复前，相对目标路径在约 4 秒内稳定返回 `ENOENT`；修复后同一命令进入实际编译，
   20 秒短验证仅由测试脚本主动超时。
3. 基于干净 `os/sdcard-la-pub.img` 的 qcow2 overlay 完整运行 final：

```text
BUILDSTORM_TOOLCHAIN ok
BUILDSTORM_MINIBUILD ok
Finished `release` profile [optimized] target(s) in 118m 25s
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=7354.13 cores=8 bytes=1714568 arch=loongarch64
```

CAgent 10/10 通过，bringup 报告 `all commands finished`。qcow2 执行 `qemu-img check`
无错误。原始镜像 SHA-256 为
`cf8660bdc216d3dd6c82f4b50cdc4271d1be6dc49eb647ccbb9a0f24f36ad245`，测试未修改原始镜像。
完整串口日志 SHA-256 为
`1026bc523f21c6baaa9f30e814852df445819fdd425a72dc0578f793c45ebff3`。

## 新发现问题

合并 overlay 后执行 `e2fsck -fn`，发现第 11 块组仍带 `INODE_UNINIT`，但 BuildStorm
已在其中分配约 744 个 inode，同时存在对应块位图差异。该问题属于 another_ext4 的
延迟 inode 位图初始化缺失，与本次 `getcwd` ABI 修复独立，必须作为下一项文件系统
一致性任务修复。fsck 日志 SHA-256 为
`be454105b7e83f16474c80a58cb6089b775a366ad4f40e3c60e6e1f11ac5ad5b`。
