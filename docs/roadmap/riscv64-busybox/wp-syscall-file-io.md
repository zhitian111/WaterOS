# 工作包：wateros-syscall — 文件类系统调用（open/read/write/close/dup 族）

**所属**：`os/components/wateros-syscall`、`wateros-vfs`、`wateros-fs`（路径解析与挂载视图）。  
**并行度**：依赖 **fd 表**；与 **内存类 syscall** 可部分并行（不同工程师），但 **`write` 到文件** 需 fd 完成。

## 要做什么

1. 在 `dispatch_syscall_from_trap`（或等价分层）中实现（对齐 `wateros-abi` **`impl-linux-generic64`** 号表）：
   - `openat` / `open`（可先只支持绝对路径）
   - `read`、`write`（**任意合法 fd**，不再仅限 1/2；1/2 默认仍可映射控制台直至 char 设备就绪）
   - `close`
   - `lseek`（若 BusyBox/基础脚本需要）
   - `dup`、`dup3`（至少 `dup`）
2. 用户指针参数须经 **MM 安全拷贝** 路径访问（`copy_from_user` / `copy_to_user` 或项目等价 API），禁止信任用户 VA 直接 `slice::from_raw_parts` 跨边界。
3. `fstat` / `newfstatat` 中与 **路径打开** 强相关的部分可与本包拆分，但须在 `wp-syscall-mem-time.md` 或本文件交叉引用避免重复实现。

## 验收要求

- [ ] 用户态程序（由 bring-up 总线启动）可对 ext4 上 **测试文件** 完成 `open → write → close → open → read → close`，内容与预期一致。
- [ ] 非法 fd：`read`/`write`/`close` 返回 **`EBADF`**。
- [ ] `write` 到控制台与 `write` 到普通文件在 syscall 层分支清晰，日志可区分（debug 级别即可）。

## 验证方式

1. 在用户程序源码树（`user/`）增加 **最小静态 ELF**（或固定路径的内建镜像），仅调用上述 syscall；由 **`user_bringup_bus`** 在 `fs::test()` 之后、`run_first_task` 之前或作为首任务启动（顺序见总线文档）。
2. QEMU 日志：固定字符串如 `[bringup][syscall-io] PASS`。
3. 使用 `strace` 等价思路：可选记录 syscall 号与返回值（debug feature，不强制）。

## 依赖

- **上游**：`wp-vfs-fd-session.md`、`wp-mm-user-riscv64.md`（用户缓冲访问）。
- **下游**：`wp-syscall-process-exec.md`（exec 需打开脚本/二进制）、`wp-ipc-pipe-signal.md`（pipe fd）。

## 可并行对象

`wp-syscall-mem-time.md` 中 **与 fd 无关** 的 `gettimeofday` 等（需注意 trap 参数与寄存器约定同一套审查）。
