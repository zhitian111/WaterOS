pub enum PrivLevel {
    Kernel,
    User,
}

pub trait PrivLevelRead {
    fn is_kernel(&self) -> bool;
    fn is_user(&self) -> bool;
}

pub trait PrivLevelWrite {
    /// 只是设置特权寄存器的权限位，需要结合 sret 等指令来做特权级切换
    fn set_privilege(level : PrivLevel);
    fn set_to_user() { Self::set_privilege(PrivLevel::User); }
}

pub trait PrivLevelFrameView: PrivLevelWrite + PrivLevelRead {}
