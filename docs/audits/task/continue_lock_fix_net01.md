# 锁审计修复接续（NET-01）

> 在新对话中复制下方「Agent 提示词」整段，交给下一个 Agent 继续本任务链。

---

## Agent 提示词

```
继续 WaterOS 锁机制审计修复，按既定顺序推进 **NET-01**（FD-01、PLAT-01/02 已完成）。

## 背景
- 单核多线程；修复须零副作用，不引入新问题；每完成一项先 `make rv_check`（必要时 `make la_check`）验收。
- **不要** git commit，除非我明确要求。

## 当前任务：NET-01
- **问题**：`NETWORK_STACK` 全局 `spin::Mutex` 长临界区；syscall 路径（read/write/connect/accept/recvfrom/poll）在持锁或锁内驱动 smoltcp/VirtIO，易阻塞其他网络/调度路径。
- **目标**：锁外 I/O 或缩短临界区；曾尝试抽 `network_drive.rs` + `stack::socket_poll_revents` 但未完成并已回滚——请重新设计并实现，避免半成品。
- **暂缓项（本轮不做）**：PLAT-03/05、F-2 futex requeue、FD R-PT-02/03。

## 必须先读
1. `docs/audits/lock-issues.md` — §9 暂缓项、§4.5 PLAT、变更摘要
2. `docs/audits/locks/` — 若有 network 相关审计则读；否则读 `lock-inventory.md` 中网络相关条目
3. `os/components/wateros-driver/driver-network/src/lib.rs` — `NETWORK_STACK`、`poll_socket_events`、socket API
4. `os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs` — `drive_network_stack`、`poll_socket_revents`
5. `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/read.rs`、`write.rs`、`connect.rs`、`accept.rs`、`recvfrom.rs` — `drive_network_stack` 调用点
6. `docs/audits/syscall/signal-socket-poll.md`（若存在 socket/poll 审计）

## 验收
- `cd os && make rv_check`（通过为准）
- 更新 `docs/audits/lock-issues.md`：NET-01 状态、代码变更摘要；从 §9 暂缓移除或注明已修复
- 可选：`make rv_qemu_run` 看网络/poll 相关日志

## 已完成（勿重复）
- FD-01：共享 fd 表 `io_inflight` + 槽位锁；`VfsError::Busy`；`clone.rs` CLONE_FILES
- PLAT-01/02：`freeze_platform_probe`；probe 锁外注册；`INIT_AFTER_BOOT_DONE` 幂等
- 构建已通过 rv_check + la_check
```

---

## 任务顺序（全链参考）

| 顺序 | ID | 状态 |
|------|-----|------|
| 1 | FD-01 | 已完成 |
| 2 | PLAT-01/02 | 已完成 |
| 3 | **NET-01** | **当前** |
| — | PLAT-03/05、F-2 requeue、FD R-PT-02/03 | 暂缓 |
