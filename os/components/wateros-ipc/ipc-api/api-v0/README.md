# wateros-ipc-api-v0 离线开发手册

本 crate 是 WaterOS IPC 的顶层版本化 API 预留边界。当前源码只有测试依赖图用的
`add(u64,u64)`，它不是 IPC 功能、不是 syscall、不是稳定 ABI，也没有任何内核对象状态。
真实 pipe/futex/shm/signal 契约目前分别位于各子系统 API crate，等待调度契约位于 waitqueue
API。整体设计见 [wateros-ipc](../../README.md)。

## 当前真实状态

```text
wateros-ipc::api
→ 重导出 wateros-ipc-api-v0
→ 当前仅 add() 占位

wateros-ipc::{pipe,futex,shm,signal,waitqueue}
→ 各自重导出独立 api-v0 + impl
→ 这里才有实际内核契约
```

因此：

- 不要让新 syscall 调用 `ipc::api::add`；
- 不要把 crate 能编译当作某种通用 IPC 已实现；
- 不要把 Linux syscall number、用户指针或架构寄存器布局放进本 crate；
- 删除占位函数前先 `rg "ipc.*add|api_v0::add"`，确认没有测试/示例依赖。

## 适合放入本 API 的内容

只有真正跨多个 IPC 子系统、且需要稳定版本化的通用值类型才适合进入本层，例如：

- 纯领域错误的共同子集（前提是不会抹平各对象差异）；
- 内核对象 ID 的透明 newtype；
- 与 syscall ABI 解耦的权限/所有者快照；
- 多子系统共享的无借用、无锁 guard、无用户指针的数据结构。

不适合放入：

- `SyscallArgs`、`ErrNo`、`copy_from_user`；
- task scheduler、页表、VFS fd 的具体类型；
- 某实现 crate 的 mutex/registry/Arc 内部状态；
- Linux `repr(C)` 结构未经独立 ABI 版本设计的直接复用；
- 只由 pipe 或 futex 使用的类型（应留在对应子 API）。

顶层“统一”不能以丢失语义为代价。pipe 的 EOF/BrokenPipe、futex 的 timeout/value mismatch、
SHM 的 removed/permission、signal 的 disposition/pending 不是同一种错误状态，不应全部折叠成
`IpcError::Failed`。

## 版本化与 ABI 原则

本 crate 名称含 v0，但目前并无稳定用户 ABI。首次加入真实类型时逐项决定：

1. **内核契约还是用户 ABI**：内核 trait/type 可自由使用 Rust enum；用户结构必须固定布局、
   宽度、端序、对齐和 padding。
2. **所有权**：跨锁边界只返回 owned snapshot、ID 或 Arc 风格 handle，不返回 guard/借用内部
   Vec。
3. **错误层次**：API 返回领域错误；syscall 层按具体操作映射 `ErrNo`。
4. **并发语义**：方法是原子状态转移、两阶段 reservation，还是只读快照，必须写清。
5. **生命周期**：fork/clone/exec/exit/close 谁复制、清除、提交和回滚。
6. **版本升级**：破坏性布局变更新建 api-v1，而不是静默改变被用户 copy 的结构。

如果类型只在内核内部使用，不要因为 `#[repr(C)]` 就宣称是 Linux ABI。反之，真实 ABI 还需
`size_of/offset` const assertion 和两架构测试。

## 新增跨 IPC 对象 ID 的实例

假设要引入只用于内核注册表的通用 ID：

```rust
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IpcObjectId(u64);

impl IpcObjectId {
    pub const fn new(raw: u64) -> Self { Self(raw) }
    pub const fn raw(self) -> u64 { self.0 }
}
```

接下来必须：

1. 定义 ID 作用域（全系统、namespace 内，还是某 registry generation 内）。
2. 定义回绕/复用和 stale ID 检测；裸递增数不能防 ABA。
3. 各子系统只在确实需要统一查询时采用，不强制替换语义更强的 `ShmId/FutexKey`。
4. syscall 仍验证用户整数、权限和 namespace，再转换成领域 ID。
5. 测试删除后 stale ID、并发创建/删除、fork/namespace 和回绕边界。

若没有两个以上子系统的真实消费者，就把 newtype 留在子 API，避免建立无用抽象。

## 添加 IPC syscall 的正确分层

```text
syscall API：编号、参数寄存器、用户 repr(C)、ErrNo
→ syscall impl：copy 用户内存、flags/权限、current task/address-space
→ ipc 子 API：领域参数、结果、reservation/dispatch intent
→ ipc impl：对象锁与 registry 状态转移
→ waitqueue/task 或 MM/VFS（锁外执行）
```

本顶层 API 不应成为 syscall 实现的万能转发函数。新功能属于 event、pipe、futex 等明确对象时，
先在对应子 API 落契约，再由聚合 facade feature 重导出。

## 占位 API 清理清单

- [ ] 仓库中不存在 `add` 的真实消费者。
- [ ] 删除 `add` 与模板单测，替换为真实类型/布局/行为测试。
- [ ] 顶层 `wateros-ipc::api` 的文档不再宣称占位。
- [ ] 真实公共类型有所有权、锁、错误、生命周期和版本说明。
- [ ] 用户 ABI 与内核领域类型分开。
- [ ] RV/LA 的 size/alignment 和 syscall 端到端测试覆盖。

## 验证

```bash
cd os
cargo test --manifest-path components/wateros-ipc/ipc-api/api-v0/Cargo.toml
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

单独 API 测试只能证明纯类型/布局；任何涉及 task/MM/VFS 的 IPC 语义必须通过顶层 feature 图和
用户态 syscall 测试证明。

