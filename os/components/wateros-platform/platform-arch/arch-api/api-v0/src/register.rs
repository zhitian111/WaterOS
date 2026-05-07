//! 寄存器组索引访问与控制寄存器（如 PC/SP）的抽象，便于与 trap 帧实现分层组合。

use core::option::Option;

/// 按 GPR 索引只读（返回 `None` 表示索引越界或不可读）。
pub trait RegisterBankRead {
    fn read_gpr_by_index(&self, idx : usize) -> Option<usize>;
}

/// 按 GPR 索引写入（具体语义由实现定义，常见为调试或占位）。
pub trait RegisterBankWrite {
    fn write_gpr_by_index(&mut self, idx : usize);
}

/// 控制流相关寄存器的只读视图（如 `sepc` / 用户栈指针）。
pub trait ControlRegRead {
    fn user_pc(&self) -> usize;
    fn user_sp(&self) -> usize;
}

/// 控制流相关寄存器的写入。
pub trait ControlRegWrite {
    fn set_user_pc(&mut self, pc : usize);
    fn set_user_sp(&mut self, pc : usize);
}
