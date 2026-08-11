# K-09：Trap、TLB/ASID 与用户访问热路径

## 任务目标

只在 K-04 证明 trap、TLB flush 或 page walk 是 Top 3，且 K-02/K-07 的 SMP、ASID 和
地址空间生命周期已通过后，降低 lmbench syscall/read/write 延迟。每个高风险机制
独立 feature、独立提交和独立消融。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-platform.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/todo/perf-hotpath.md`
- `docs/todo/perf-memory.md`
- `docs/todo/perf-risk-assessment.md`
- `docs/tasks/perf/wave3-trap-tlb-hotpath/task.md`
- `docs/tasks/read-family/02-user-copy-progress.md`

## 已知信息与代码证据

- LoongArch trap 返回已携带 PGDL+ASID，汇编注释明确避免每次清空其它地址空间 TLB。
- RISC-V 已探测 `satp.ASID` WARL 位并在支持时走快速切换，但 fallback 和部分激活
  仍执行全局 `sfence.vma`：

```rust
pub fn activate_address_space_token_and_flush(token: usize) {
    unsafe {
        asm!("csrw satp, {0}", in(reg) token);
        asm!("sfence.vma x0, x0");
    }
}
```

- RIO-02 会修改 copy fault progress。K-09 可以合并软件 page walk，但不得改变部分
  成功、权限和 COW 语义。
- 历史 G7 数据显示 syscall/read/write/stat/open 卡 baseline；stat/open 还依赖 K-05，
  不能把所有延迟归因于 trap。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/paging.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/{asm,src}/`
- `os/src/trap_handler.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_access.rs`
- `os/components/wateros-ipc/ipc-signal/`
- `os/components/wateros-task/task-scheduler/`
- `docs/todo/perf-{hotpath,memory,risk-assessment}.md`
- `docs/tasks/perf/wave3-trap-tlb-hotpath/task.md`

## 任务内容

按以下顺序逐项执行，禁止一次性大改：

1. **计数**：trap 次数、satp/PGDL 切换、local/remote flush、page walk 和 signal
   registry 慢路径。先分清架构差异。
2. **低风险快路**：无 pending signal 时跳过 registry；同一 kernel/user token 下
   避免重复 activate。保持 signal memory ordering。
3. **user walk**：将 translate 与 permission 检查合并成一次 walk，沿用 RIO-02 的
   copy progress 返回；跨页、COW、lazy fault 和 concurrent unmap 必须保持。
4. **ASID/selective flush**：设计 generation、reuse、每 CPU active-aspace mask 和
   remote shootdown。ASID 复用前必须使所有 CPU 上旧 generation 失效。
5. **trap context copy**：只有 profile 证明多重 TrapContext copy 占比高时，才收敛
   到唯一稳定缓冲；嵌套 kernel fault、signal frame、首次用户返回和 AP 启动必须
   单独验证。
6. 每项 feature 默认旧路径，先单核、再 8 核、再 LTP/benchmark；发现静默错页立即
   关闭 feature，不在同一提交继续叠加优化。

选择性 flush API 必须携带作用域，而不是让调用方拼汇编：

```rust
enum TlbFlushRange {
    Address { token: usize, va: usize },
    AddressSpace { token: usize },
    All,
}
```

可沿用仓库已有类型；示例只说明契约。

## 如何验收

- [ ] 每项独立 feature 和提交，默认旧路径可构建运行。
- [ ] 两架构 `make *_check`，flag 开/关均通过。
- [ ] 修改前后三轮 lmbench Simple syscall/read/write，有超过噪声的改善。
- [ ] signal、mmap/mprotect/munmap、fork/COW、exec、user-copy fault LTP 通过。
- [ ] 8 核 ASID reuse/TLB shootdown 压测无旧映射、错页、UAF 和跨进程泄漏。
- [ ] 嵌套 trap、kernel fault、首次用户返回和远端 IPI 路径有定向测试。
- [ ] 计数证明全局 flush/page walk/registry 慢路下降，收益能由单项消融重现。

结果写入 `docs/tasks/history/known-issues/k09-<feature>-YYYYMMDD.md`。收益小于噪声或
只能在单核成立的改动不进入最终候选。
