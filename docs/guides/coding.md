# WaterOS 编码规范

本文件面向项目成员，描述 WaterOS 当前推荐采用的编码规则与案例。

## 总原则

- 优先保持组件边界清晰，再追求局部实现速度。
- API 与 impl 职责明确分层，避免在实现层直接重写接口语义。
- 公开接口优先保证命名稳定与文档完整。
- 变更 feature、导出链和聚合层时，必须同步更新相关文档。

## Rust 代码

### API 层

要求：

- 在 `api-v0` 中定义 trait、类型、错误和常量。
- 不在 API 层编写具体平台逻辑。
- `pub` 接口优先补齐 `///` 注释。

示例：

```rust
/// 提供平台计时能力的抽象接口。
pub trait PlatformTime {
    fn time_frequency_hz() -> Result<u64, PlatformTimeError>;
}
```

### impl 层

要求：

- 只实现已定义契约。
- 若实现仍为临时版本，要写清限制和替换方向。
- 平台或硬件细节集中在 impl 中，不泄漏到聚合门面。

示例：

```rust
/// 临时实现，仅用于当前 bring-up 阶段。
pub struct StackFrameAllocator {
    recycled: Vec<PhysPageNum>,
}
```

### 聚合层

要求：

- 使用稳定的导出名。
- 通过 `cfg(feature = ...)` 选择实现。
- 把最终对外接口收敛到聚合 crate。
- 若组件需要自检，优先在聚合层提供统一的 `test()` 或 `test_with_xxx(...)` 入口，而不是让上层直接调用具体 impl 内部测试函数。
- 统一测试入口应负责串联 API 层、子组件层和当前激活 impl 的测试逻辑。

示例：

```rust
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi as active_impl;
```

## 统一测试接口

WaterOS 当前推荐通过聚合层的统一测试接口组织组件自检。

要求：

- 无上下文输入的组件优先导出 `test()`。
- 需要额外上下文的组件使用语义明确的测试入口，例如 `test_with_range(...)`。
- 上层只依赖聚合层测试入口，不直接依赖某个 impl 私有测试函数。
- 若当前激活 impl 为占位实现，也应在统一测试入口中明确记录跳过原因。

示例模式：

```rust
pub fn test() {
    log::trace!("[driver] test begin");
    api_v0::test();
    block::test();
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::test();
    #[cfg(feature = "self_test")]
    log::info!("[driver] self_test: skip hardware-dependent probe when unavailable");
    log::trace!("[driver] test end");
}
```

这个模式的重点不是“有一个测试函数”本身，而是由聚合层统一组织测试链，保证上层调用方式稳定。

## Logging 编写规范

WaterOS 当前日志应统一走 `runtime-logging` 初始化后的 `log` 宏体系。

要求：

- 统一使用 `trace!`、`debug!`、`info!`、`warn!`、`error!`。
- 日志前缀要体现组件身份，例如 `[driver]`、`[wateros-mm]`。
- `trace!` 用于路径开始、结束和细粒度调试信息。
- `debug!` 用于输出关键中间状态或调试值。
- `info!` 用于阶段性成功、初始化完成和可接受的降级说明。
- `warn!` 用于可恢复异常或非致命失败。
- `error!` 只用于明确错误路径，不滥用。
- 测试和初始化日志尽量成对出现，便于从串行输出中判断执行进度。

示例：

```rust
log::trace!("[wateros-mm] test begin");
log::info!("[wateros-mm] dummy impl: no mm-impl test");
log::trace!("[wateros-mm] test end");
```

## Cargo.toml

要求：

- 新增 impl 时同时补齐 workspace、依赖和 feature 传递关系。
- feature 名保持语义清晰。
- 默认 feature 仅指向项目认可的默认实现。

反例：

- 新增 impl 目录但未加入 workspace members。
- 聚合层启用了某 feature，但子组件没有向下传递。

## Makefile 与脚本

要求：

- 命令名要清晰、职责单一。
- 涉及生成产物时，要说明产物位置。
- 修改构建入口时应同步更新相关文档。

## 文档文件中的代码片段

要求：

- 示例代码必须与当前目录结构和命名一致。
- 不使用虚假的 crate 名和 feature 名。
- 若示例是规划性的，应明确标注为“建议示例”。
