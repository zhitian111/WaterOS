use core::option::Option;
pub trait RegisterBankRead {
    fn read_gpr_by_index(&self, idx : usize) -> Option<usize>;
}
pub trait RegisterBankWrite {
    fn write_gpr_by_index(&mut self, idx : usize);
}

pub trait ControlRegRead {
    fn user_pc(&self) -> usize;
    fn user_sp(&self) -> usize;
}

pub trait ControlRegWrite {
    fn set_user_pc(&mut self, pc : usize);
    fn set_user_sp(&mut self, pc : usize);
}
