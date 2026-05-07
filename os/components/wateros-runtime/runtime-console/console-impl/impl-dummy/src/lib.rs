#![no_std]
//! 控制台 dummy 实现：用于无真实串口或未链固件后端的构建；**任何写入会触发 `unimplemented!`**。

/// 占位控制台句柄；`write_str` 当前为 `unimplemented!()`，仅用于类型检查与链接占位。
///
/// **后续替换点**：接入真实 `Console` 实现或测试用 sink 时应移除此路径。
#[derive(Default)]
pub struct DummyConsoleHandle;
impl core::fmt::Write for DummyConsoleHandle {
    #[allow(unused)]
    #[allow(unused_variables)]
    fn write_str(&mut self, s : &str) -> core::fmt::Result { unimplemented!() }
}
impl api_v0::Console for DummyConsoleHandle {}
