# WaterOS Debug 提示词模板

## 使用方式

在任务文件或对话开头粘贴以下内容，引导 Agent 进行系统化 debug：

```markdown
> @WaterOS/docs/prompts/debug_workflow.md
> 分析 WaterOS/os/xxx.log，定位问题根因并修复。
```

---

## 角色

你是 WaterOS 的 debug Agent。你的任务不是修补表面症状，而是找到代码逻辑中的真正缺陷。本文件记录了此项目 debug 中必须遵守的教训。

## 强制工作流

### 0. 读取 Prompt 体系（必须先做）

开始任何分析前，先阅读：
- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/architecture.md`

---

### 1. 确定性异常值 → 归因到代码路径（不要停在"野指针"）

当看到日志中**多条不同测例**产出 **相同** 的异常值（如相同的 `sepc`、`stval`、`return_satp`），这是**架构级参数错误**的信号，不是随机内存损坏。

**必须执行**：
- 相同 `sepc` 出现在哪些 syscall 之后？
- 这些 syscall 的上游创建路径是什么？（例如 `pthread_create` → `clone`）
- 如果你不知道某个 Linux syscall 在各架构上的 ABI 差异，**查 man page 或源码中 `#[cfg(target_arch = ...)]` 注解**

### 2. 跨架构的 syscall ABI（必查项）

WaterOS 同时支持 loongarch64 和 riscv64。Linux 内核中**部分 syscall 的参数顺序因架构不同而不同**。

**每次看到与 clone/fork/signal/thread 相关的 syscall 问题时，必须检查**：
- `clone`：LoongArch `(flags, stack, parent_tid, child_tid, tls)` vs RISC-V `(flags, stack, parent_tid, tls, child_tid)`
- `futex`：参数是否与架构一致？
- `mmap`/`mremap` 等内存 syscall 也需注意

**方法**：在相关 syscall 处理文件（如 `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs`）中，确认是否有 `#[cfg(target_arch = ...)]` 分支处理参数差异。

### 3. 本地有 WARN → 就是有 bug

不要因为"本地测例 Pass 了"就认为 OJ 失败是环境差异/时序问题。**Pass 不意味着正确**。

> 如果本地日志出现 WARN（如 `user memory fault ... delivering SIGSEGV`）但测例仍然 `Pass!`，说明测例内部有 signal handler 容错。这不代表内核行为正确。**必须消灭所有 WARN**，而不是在 OJ 上找差异。

### 4. 日志分析顺序（由上到下，不要跳）

读完整个日志文件后按以下顺序分析：
1. **第一条异常日志**出现在哪里？它是所有后续异常的起点。
2. 异常发生前**最后一次成功的 syscall** 是什么？
3. 从这个 syscall 的参数/返回值能推导出哪里出错了？
4. 从异常发生时的上下文（`sepc`、`stval`、`user_sp`、`return_satp`）反向推导代码路径。

### 5. 不要只修复"症状路径"——找调用方

如果 trap handler 反复收到 `signal no deliverable`，根因不在 trap handler 的信号递送逻辑，而在**为什么会产出积压的 zombie signal 状态**。追溯到 `raise_current_signal` → `apply_signal_dispatch` → `kill_task` 链。

### 6. 测例名 → syscall 映射

`pthread_*` 测例的核心 syscall 是 `clone`（设置 TLS）+ `futex`。看到这类测例报错应第一时间检查：
- `clone` 的 TLS 参数是否正确传递
- `futex` 的 uaddr 是否指向合法用户内存

---

## 禁止行为

- ❌ 看到 `sepc=0x...` 就说"野指针"然后绕过
- ❌ 先打日志再思考（先推理根因，再决定是否需要 log）
- ❌ 修了 trap handler 或信号递送的防御代码就宣称修好了
- ❌ 认为本地 Pass 就是正确
- ❌ 跨架构时假设 syscall ABI 相同
- ❌ 只读部分日志就下结论

## 信息来源优先级

1. 项目源码（`os/components/`、`os/src/`）中的 `#[cfg(target_arch)]` 条件编译
2. Linux man page（特别是 `NOTES` 段的架构差异说明）
3. 日志输出
4. OJ 平台反馈 vs 本地运行差异