# wateros-platform 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时必须同时检查 `wateros-platform/Cargo.toml`、`platform-impl`、`platform-arch`、`platform-firmware` 的 feature 传递链。实现中通常需要定义 BootArgs、时间能力和固件调用桥接。

新增 arch impl 时，应优先接入 `platform-arch` 的 `api-v0` 契约，并同步检查任务系统是否仍直接依赖某个具体 ISA。当前 `platform-arch` 的 active impl 通过 `impl-riscv64` 或 `impl-loongarch64` feature 选择；具体实现需要提供时间读取、全局/时钟中断控制、任务切换上下文、trap frame 语义读写，以及 `__switch`、`__arch_task_entry`、`__arch_user_task_entry`、`__wateros_arch_restore_user_task` 等任务机制符号。

`impl-loongarch64` 当前已经具备 QEMU virt 的基础平台路径、trap/switch 汇编、timer interrupt 与 PLV3 syscall smoke；该 smoke 仍是链接进内核镜像的 `.text.user_smoke` 段，并通过 `UserTaskSpec`/observer 验证 task 资源快照，不是从根卷解析出的真实 ELF。要让 LoongArch 跑真实 ELF 用户任务，需要继续补齐 LoongArch MMU/页表实现、driver/fs 挂载路径，以及对应的 `kernel_mm::from_elf_path` 装载实现。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
