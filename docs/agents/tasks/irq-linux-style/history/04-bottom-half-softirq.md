# T04 任务简报：bottom-half / softirq + irqaction 线程化

- 完成日期：2026-08-15
- commit：`a83d717a`（`[feat] 引入 IRQ bottom-half 框架与内核任务`）
- 前置：`407b9225`（T03 LoongArch EIOINTC 链路）

## 实际改动摘要

`wateros-irq`：

- 新增 `bottom_half.rs`：固定容量（64）环形队列 + `set_wake_callback` /
  `schedule` / `has_pending` / `run_pending` / `pending_len`。
  ISR 侧（trap 上下文，本地中断已屏蔽）只 push 并调用已注册唤醒回调；任务侧
  pop 后释放锁再执行 handler，避免 handler 内注册操作自死锁。
- `action.rs`：
  - 新增 `BottomHalfFn = fn(Virq, usize)`，`IrqAction` 增加 `bottom_half` 字段；
  - `request_irq` 保留（无 bottom-half），新增 `request_irq_with_bottom_half`；
  - `dispatch` 在 top-half 返回 `Handled` 且存在 bottom-half 时自动
    `bottom_half::schedule`；
  - 新增 `run_bottom_half`（按 virq+dev_id 找首个匹配 action 执行 bh）。
- `lib.rs`：`pub mod bottom_half`；self_test 与单元测试覆盖
  dispatch→schedule→run_pending 全链路。

内核侧（`os/`）：

- 新增 `irq_bottom_half.rs`：`Once<WaitQueue>` + 唤醒回调 +
  `extern "C" fn bottom_half_task`（`run_pending` → `wait_current_while(无待处理)`，
  调度器临界区复查避免丢失唤醒）；`init()` 注册回调并 `spawn_kernel_task`。
- `os/Cargo.toml` 增加 `spin` 依赖（顶层使用 `spin::Once`）。
- `main.rs`：`mod irq_bottom_half`，`init_after_boot` 在 `task::init()` 后调用
  `irq_bottom_half::init()`（两架构共用）。

## 验证命令与结果

- `cd os/components/wateros-irq && cargo test`：2 passed
  （core_roundtrip + bottom_half_roundtrip）。
- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过。
- RISC-V QEMU smoke（QEMU 9.2.1 + `-snapshot`）：
  - `[irq] bottom-half task started` 出现；
  - `[fs] init end`、cagent_testcode.sh `exit_code=0`；
  - 无 panic / fatal / `[irq] unhandled`。
- `git diff --check`：干净。

## 未验证项 / 风险 / 下一步

- bottom-half 尚未被真实设备 IRQ 触发（T06 块异步接入后才首次实战）；环形队列
  满返回 `false` 的背压语义与 `run_pending` 任务侧持锁窗口（理论上的中断重入
  窗口）在 T06 启用设备 IRQ 时需复核，必要时在临界区掩本地中断。
- LoongArch 运行 smoke 仍受镜像磁盘空间限制（同 T03）。
- 下一步 T05：BlockDevice 契约改 `&self` + 内部锁 + 同步回退（不改变 I/O 完成
  方式，为 T06 异步化铺路）。
