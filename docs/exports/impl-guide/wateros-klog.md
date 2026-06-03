# wateros-klog 新增 impl 指南

## 用途

指导首次落地 **`wateros-klog`** 或后续新增存储后端（例如另一套 `klog-impl/*`）时的检查清单。设计细节见 **[`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)**。

## 首次落地步骤

### 1. 创建组件树

```text
os/components/wateros-klog/
  Cargo.toml
  src/lib.rs
  klog-api/api-v0/Cargo.toml + src/
  klog-impl/klog-ringbuf/Cargo.toml + src/
```

- 将各子 crate 加入 **`os/Cargo.toml` workspace members**（与 `wateros-ipc`、`wateros-mm` 相同模式）。
- **不要**嵌套独立 workspace root。

### 2. `klog-api/api-v0`

- 定义 `KlogRecordMeta`、`KlogFlags`、`KlogStore`、`SyslogAction`、错误类型。
- 所有 `pub` 项补齐 `///` 契约（字段含义、并发假设、不导出用户态等）。
- 仅依赖 `no_std` 友好 crate（如 `wateros-base`）。

### 3. `klog-impl/klog-ringbuf`

- 实现 `KlogStore`：desc 槽 + `KLOG_TEXT_RING_BYTES` 变长环。
- 全局单例 + **spin mutex**。
- 环满：**覆盖最旧** + `records_dropped`（见架构文档）。
- `read_cursor` 与 `CLEAR` / `READ_CLEAR` 语义：**mark_read**，不物理擦除。

### 4. 聚合 `wateros-klog/src/lib.rs`

- `cfg` 选择 `klog-ringbuf` 为 `ActiveKlogStore`（与 `ActiveSyscallNumberTable` 模式类似）。
- 导出 `init`、`record`、宏、`export::*`、`syscall::dispatch`。
- 依赖 `platform` 取 `ts_nsec`。

### 5. `wateros-base-config`

- 增加 `KLOG_DESC_SLOTS`、`KLOG_TEXT_RING_BYTES`、`KLOG_MAX_RECORD_BYTES`。
- 文档同步 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)。

### 6. `wateros-abi`

- `SyscallNumberTable::SYSLOG = 116`。
- `wateros-syscall` 的 `SyscallKind::Syslog` + `decode` 分支。

### 7. `wateros-syscall`

- 新增 `sys/syslog.rs`：`sys_syslog` → `klog::syscall::dispatch`。
- `user_copy` 读写用户缓冲。
- 测试期：未实现 action **panic**（与架构文档一致）。

### 8. 根 `wateros`

- `Cargo.toml` 增加 `klog` 依赖与 feature 传递。
- `os/src/main.rs`：在 `runtime::logging::init()` **之前**调用 `klog::init()`。

## 通用检查清单

- [ ] workspace members 已更新
- [ ] 无 `wateros-runtime-logging` → `wateros-klog` 依赖（保持零耦合）
- [ ] `wateros-syscall` 仅通过 `klog::syscall` 模块访问环
- [ ] `klog-api` rustdoc 与 `docs/exports/public-api/wateros-klog.md` 一致
- [ ] busybox / 116 号测例不再 `unknown nr=116` panic
- [ ] 更新 `docs/exports/features/wateros-klog.md` 实现状态表
- [ ] 更新 `docs/architecture/snapshot.md` 与 `docs/prompts/structure.md`

## 新增第二套存储 impl（未来）

若增加例如 `klog-impl/impl-linear`：

1. 新 impl crate 仅依赖 `klog-api`。
2. `wateros-klog/Cargo.toml` 增加 feature `impl-ringbuf` / `impl-linear`。
3. 聚合 `src/lib.rs` 用 `cfg` 绑定 `ActiveKlogStore`。
4. 同步 `docs/exports/features/wateros-klog.md` 与 impl-guide 本节。

## 维护要求

子 crate 或 feature 链变化时，同步更新本文件与 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)。
