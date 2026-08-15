# T06-f：virtio 多请求在途描述符回收不变量修复

## 背景

多请求在途（8 槽位 async + nb 提交 + 按 used 顺序回收）下，`complete_*` 会返回
WrongToken，表现为 `failed to read block ... IoError`。证据见
`history/06e-fs-lock-free-read-path.md` 追加节。

## 任务内容

1. 审计 vendor `virtio-drivers/src/queue.rs` 的 `add`/`add_indirect`/
   `recycle_descriptors`/`pop_used` 在乱序完成 + 描述符复用下的 free-list 不变量。
2. 找出重复 token 的产生点（同一描述符被两个在途槽位同时持有）并修复。
3. 修复后重新启用 8 槽位 async 路径，用 cagent 全量 + buildstorm 验收。

## 涉及文件

- `os/vendor/virtio-drivers/src/queue.rs`
- `os/vendor/virtio-drivers/src/device/blk.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`

## CodeGraph / 检索

```bash
rg -n "fn add_indirect|fn recycle_descriptors|fn pop_used|free_head" os/vendor/virtio-drivers/src/queue.rs
```

## 验收

- 多请求在途 + IRQ 模式 cagent 10/10 `pass` + `command succeeded exit_code=0`；
  buildstorm `ok=true`；无 IoError/PANIC。

## 任务简报

完成后写 `history/06f-virtio-multiqueue-descriptor.md`。
