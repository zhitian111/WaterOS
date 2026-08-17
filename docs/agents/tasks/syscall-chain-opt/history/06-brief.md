# RISC-V 性能回归日志级别简报

## 完成状态

第一批优化基线保持在 `51a19a75`；第二批路径解析、lazy ELF 和条件 TLB flush 提交已从当前
分支回退。RISC-V 平台 profile 的编译期最大日志级别单独从 `Info` 调整为 `Warn`，避免热路径
日志干扰性能测量。LoongArch64 继续使用 `Error`。

## 验证

```bash
cd os
make rv_check
make kernel-rv
```

QEMU 性能回归继续要求 QEMU 9.2.1、8 vCPU、16 GiB 和 `-snapshot`。
