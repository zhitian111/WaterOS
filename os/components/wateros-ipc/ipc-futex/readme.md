# ipc-futex

`ipc-futex` 维护 futex 等待队列和 per-task robust 状态，不解析 syscall ABI，也不直接
读写用户内存。

```text
futex-api/api-v0
  FutexKey、错误、等待结果、robust 用户态布局
          │
          ▼
futex-impl/impl-task
  registry：等待队列与 robust 侧表
  global：隐藏全局锁的模块级 wait/wake/requeue/robust facade
          │
          ▼
ipc-futex
  薄聚合层，只重导出 API 和当前实现
```

## 边界

- API 层不依赖 task，不保存全局状态。
- impl 层通过 `ipc-waitqueue` 完成阻塞、唤醒和 requeue；registry 与锁不对外暴露。
- syscall 层负责操作码、超时和 errno 转换，以及 futex 用户字和 robust 用户链表访问。
- 调用方直接使用 `ipc::futex::wait_while()`、`wake()`、`requeue()` 等模块函数。

`wait_while()` 在取得队列前后复查用户态条件，队列的 `active_users` 覆盖取得
`WaitQueueId` 到 scheduler 操作完成的窗口，避免空队列被释放后 ID 立即复用。
private futex 使用“地址空间 + 用户虚拟地址”作为 key；shared futex 由 MM
先解析为物理字身份，因此同一共享页映射到不同进程或不同虚拟地址时仍能互相唤醒。

## Robust 生命周期

- `set_robust_list` 登记当前线程的用户链表头。
- 普通查询使用 `get_robust_list`。
- 线程退出使用 `take_robust_list` 一次性取出并删除状态，再由 syscall 层遍历用户链表、
  设置 `FUTEX_OWNER_DIED` 并唤醒等待者。
- reap/失败回滚可使用 `drop_robust_list` 做幂等清理。

## 当前限制

- `FUTEX_WAIT_BITSET` / `FUTEX_WAKE_BITSET` 仅支持 `FUTEX_BITSET_MATCH_ALL`。
- PI futex 尚未实现；robust 链表中带 PI 标记的节点会跳过，避免按普通 futex
  错误清理。
