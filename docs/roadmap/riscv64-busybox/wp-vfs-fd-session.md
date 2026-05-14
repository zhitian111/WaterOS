# 工作包：wateros-vfs — per-task 文件描述符与会话

**所属**：`os/components/wateros-vfs`（及与 `wateros-task` 的任务结构体挂载点）。  
**并行度**：可与 **mm 用户地址空间** 并行设计；**实现**建议在 mm 第一版映射契约冻结后开始编码。

## 要做什么

1. 为 **每个任务**（先支持用户任务即可）维护 **fd 表**：stdin/stdout/stderr 的默认占位（0/1/2），与当前 syscall 仅允许 1/2 写控制台的行为 **演进式兼容**（最终 0/1/2 可指向字符设备或管道）。
2. 将现有 **`vfs-bridge` 烟囱能力** 接到 **打开文件描述符**：`openat`/`open` 解析路径后持有 **会话或 inode 句柄**，`read`/`write`/`lseek`/`close` 走同一抽象。
3. 定义 **与 `wateros-fs` 的边界**：哪些操作经 bridge RO/RW 会话、何时刷新缓存；与 `docs/guides/filesystem-current.md` 冲突时以代码与 exports 文档更新为准。
4. **线程模型**：当前阶段可假定 **每任务单线程**，futex 等后续再接；fd 表仍按任务粒度设计，便于后续扩展。

## 验收要求

- [ ] 两个不同用户任务（或一用户一内核辅助任务）持有 **独立 fd 表**：任务 A `open` 的文件描述符不可被任务 B 直接 `read`（除非 `dup` 显式传递，dup 属 syscall 包）。
- [ ] `close` 后相同 fd 号可复用，`EBADF` 语义正确。
- [ ] bring-up 总线阶段可在 **无完整 shell** 下：打开根卷上已知路径文件、读回魔数、关闭。

## 验证方式

1. 在 `wateros-vfs` 聚合层或 `wateros` bring-up 总线增加 **`vfs::user_fd_bringup::test()`**，在内核态模拟「当前任务上下文」下多次 open/close（若尚无用户任务，可临时用 **内核任务 + 伪造 CurrentTask 句柄** 的测试 API，**仅限 bring-up**，文档标注 TODO 移除条件）。
2. 用户路径可用后：同一总线阶段改为 **spawn 极简用户程序**（仅 open/read/close），日志断言魔数；**不**放入 `self_tests`。
3. 可选：对比 ext4 上同一文件的 RO 根句柄读与 fd 读，内容一致。

## 依赖

- **上游**：`wp-mm-user-riscv64.md`（用户上下文与拷贝进/出用户缓冲的地址验证）。
- **下游**：`wp-syscall-file-io.md`。

## 可并行对象

`wp-platform-driver-scaffold.md`；`wateros-abi`  errno 常量补齐（小改动）。
