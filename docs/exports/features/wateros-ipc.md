# wateros-ipc 功能快照

## 当前状态

当前已具备 IPC 组件和多个子方向的目录拆分，但真实可用实现仍相对有限。当前 `ipc-waitqueue` 已先行接入，作为对 `wateros-task` 等待/唤醒原语的 IPC 语义包装层，并可直接复用底层 `TaskWaitHandle`；后续重点仍在 pipe、signal、waitqueue 的逐步落地。

## 后续关注点

- 继续把 `ipc-waitqueue` 从薄包装推进为更完整的等待对象 / 事件对象抽象
- 持续补齐注释与公共 API 文档
- 在新增 impl 或公共能力变化时同步刷新本文件
