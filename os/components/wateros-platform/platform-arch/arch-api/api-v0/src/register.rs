//! 寄存器组索引访问与控制寄存器（如 PC/SP）的抽象，便于与 trap 帧实现分层组合。

use core::option::Option;

/// 按 GPR 索引只读（`None` 表示索引越界或不可读）。
pub trait RegisterBankRead {
    /// 读取第 `idx` 个通用寄存器。
    fn read_gpr_by_index(&self, idx : usize) -> Option<usize>;
}

/// 按 GPR 索引写入（具体语义由实现定义，常见为调试或占位）。
pub trait RegisterBankWrite {
    /// 写入第 `idx` 个通用寄存器。
    fn write_gpr_by_index(&mut self, idx : usize);
}

/// 控制流相关寄存器的只读视图（如 `sepc` / 用户栈指针）。
pub trait ControlRegRead {
    /// 用户态程序计数器。
    fn user_pc(&self) -> usize;
    /// 用户态栈指针。
    fn user_sp(&self) -> usize;
}

/// 控制流相关寄存器的写入（命名沿用“用户 PC/SP”语义，内核返回路径亦可能写入 `sepc` 等）。
pub trait ControlRegWrite {
    /// 设置用户态 PC。
    fn set_user_pc(&mut self, pc : usize);
    /// 设置用户态栈指针。
    fn set_user_sp(&mut self, pc : usize);
}
