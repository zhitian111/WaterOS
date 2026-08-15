# T06-e：block 读路径去 FS 持锁（睡眠安全前置）+ virtio nb+drain 完成健壮性

## 背景（T06-d 结论）

IRQ 模式用户态冻结的完整根因链（见 `history/06d-block-irq-task-sleep.md`）：

1. 用户任务在持有 FS 锁（典型 `SharedRwFs` 写锁，`open_node` 等路径）时进入
   block 读睡眠；
2. 该读在部分时序下长期不完成（virtio nb+drain 在多在途请求下的完成可见性
   问题，关闭 `RING_EVENT_IDX` + 每次强制 kick 仍复现）；
3. timekeeper CPU 上其它任务争同一把 FS 写锁 → SIE=0 无限自旋 → timekeeper
   timer 停摆 → 全局超时冻结 → 持锁者永不唤醒 → 死锁。

因此「任务睡眠」要落地，必须先保证 **block 读提交时调用方不持有任何 FS
自旋/读写锁**，并修 nb+drain 的完成可靠性。

## 任务内容

1. 审计并消除 block 读路径上的 FS 锁持有：
   - `SharedRwFs`：`open_node`/目录项读/元数据读在持写锁期间的下层块读；
   - `impl-block-cache`/page cache：T07-a 已移出 miss 读，核对其余路径；
   - ext4 `LocalRwFs` 读锁：T07-c 已改 RwLock 读并发，核对写路径与元数据读。
2. 修 virtio nb+drain 完成健壮性：
   - 对比 sync `add_notify_wait_pop` 与 nb 路径的队列状态（avail/used/num_used/
     last_used_idx 在多在途下的不变量）；
   - 候选：完成事件按 token 有序回收的边界、`peek_used`/`pop_used` 与设备
     写入的可见性、多请求在途时的描述符回收正确性。
3. 全部通过后再开 `BLOCK_IRQ_MODE_ENABLED`，以 cagent 全量 + buildstorm 验收。

## 已确认的持锁跨读路径（审计结果）

- `vfs-impl/impl-fs-bridge/src/stable_node.rs::open_stable_node`：
  `fs.write().open_node(rel)` 持 `SharedRwFs` **写锁**期间执行
  `impl-another-ext4::operations::open_node → lookup`（ext4 目录项读 → 块读）。
  GDB 现场即卡在该写锁自旋。修复方向：lookup 用读锁、`open_nodes` 计数用短
  写锁临界区，或把块读整体移出锁。
- 其余路径（page-cache miss 读 T07-a、`LocalRwFs` 读锁 T07-c）需逐一核对是否
  仍存在「持锁 → 块读 → 睡眠」。

## 涉及文件

- `os/components/wateros-fs/fs-impl/impl-another-ext4/`（元数据读路径）
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/`（`open_node`、读/写锁）
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`
- `os/vendor/virtio-drivers/src/queue.rs`、`device/blk.rs`（如确需 vendor 修复）

## CodeGraph 查询命令

```bash
codegraph explore "open_stable_node open_node read_blocks submit_async irq_bottom_half"
codegraph impact SharedRwFs
codegraph impact VirtioBlkDevice
```

## 验收方式

- 静态：`make rv_check && make la_check`。
- 单测：`wateros-irq`、`impl-block-cache`、`another_ext4 --features block_cache`。
- 开启 IRQ 开关后 cagent-glibc 10/10 `pass` + `command succeeded … exit_code=0`，
  buildstorm `BUILDSTORM_COMPILE ok=true`；无冻结、无 PANIC、无 SIGSEGV-kill。
- GDB 复查：冻结现场不再出现「timekeeper 在 FS 写锁上 SIE=0 自旋」。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final-debug
# QEMU 9.2.1 + -snapshot + pub 镜像跑 cagent/buildstorm，断言全部 pass
```

## 任务简报

完成后写 `history/06e-fs-lock-free-read-path.md`。
