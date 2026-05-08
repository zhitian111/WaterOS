#![no_std]
//! 控制台 dummy 实现：用于无真实串口或未链固件后端的构建；**任何写入会触发 `unimplemented!`**。
//!
//! **当前行为**：满足 `Console` 类型约束以便通过编译；一旦实际输出即 panic，暴露误用 dummy 的路径。
//! **后续替换点**：接入真实 `Console` 或测试用 sink 后应启用对应 feature，避免在生产内核中保留本实现。

/// 占位控制台句柄；`write_str` 当前为 `unimplemented!()`，仅用于类型检查与链接占位。
///
/// **后续替换点**：接入真实 `Console` 实现或测试用 sink 时应移除此路径。
#[derive(Default)]
pub struct DummyConsoleHandle;
impl core::fmt::Write for DummyConsoleHandle {
    #[allow(unused)]
    #[allow(unused_variables)]
    // 故意不在此处做静默吞字：无后端时尽早失败，避免误以为已输出日志。
    fn write_str(&mut self, s : &str) -> core::fmt::Result { unimplemented!() }
}
impl api_v0::Console for DummyConsoleHandle {}
