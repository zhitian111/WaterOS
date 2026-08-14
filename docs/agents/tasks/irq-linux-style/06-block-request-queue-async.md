# T06：块设备 request_queue + submit/complete + 多请求在途

## 任务内容

在 RISC-V virtio-MMIO 块驱动上实现 request_queue 语义：提交 `read_blocks_nb`/
`write_blocks_nb`，IRQ bottom-half 完成回收并唤醒等待任务，支持多请求在途。
LoongArch 本任务保持同步回退，只保 check。

## 实施方案

1. 块驱动内部维护请求槽位（token → request/buffer/completion），队列深度 ≤ VirtQueue 上限。
2. 提交后释放设备队列短锁，调用任务在 waitqueue 上阻塞；bottom-half 在 used-ring
   有序回收对应 token 后唤醒全部等待者（避免「已完成但无后续 IRQ」的丢失唤醒）。
3. 严格禁止 sync `add_notify_wait_pop` 与 async `*_nb` 混用同一队列。
4. 对外仍同步：调用方视角是「提交 → 等待完成 → 返回结果」。
5. IRQ 未就绪 / early boot / 非可等待上下文保留同步 fallback。

## 涉及文件

- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`
- `os/components/wateros-irq/src/bottom_half.rs`
- `os/components/wateros-driver/driver-block/block-api/api-v0/src/lib.rs`（如需完成契约）
- `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/register.rs`（IRQ 注册）

## CodeGraph 查询命令

```bash
codegraph explore "read_blocks_nb complete_read_blocks peek_used ack_interrupt enable_interrupts"
codegraph explore "VirtioBlkDevice from_mmio"
codegraph impact VirtioBlkDevice
```

## 验收方式

- 静态：`make rv_check && make la_check`。
- QEMU 功能：RISC-V 块 I/O smoke + iozone 最小集（读/写/重开/fsync），无竞态、无卡死。
- LoongArch 保持同步回退，check 通过。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final
make rv_qemu_run_snapshot
# 需要时用 scripts/run_iozone_minimal.sh 做定向块 I/O
```

## 任务简报

完成后写 `history/06-block-request-queue-async.md`。
