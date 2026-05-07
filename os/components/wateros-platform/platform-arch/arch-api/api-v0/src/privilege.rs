//! 特权级标记与 CSR 语义相关的最小接口（**不**表示“用户态切换”的完整协议）。

/// 当前逻辑上的执行特权级。
pub enum PrivLevel {
    Kernel,
    User,
}

/// 只读查询当前帧或上下文是否处于内核/用户语义。
pub trait PrivLevelRead {
    fn is_kernel(&self) -> bool;
    fn is_user(&self) -> bool;
}

/// 写入与特权级相关的 CSR 位；**仅**描述寄存器副作用，完整切换需配合 `sret` 等流程。
pub trait PrivLevelWrite {
    /// 仅设置特权相关 CSR 位；与 `sret` 等配合才能完成实际特权级切换。
    fn set_privilege(level : PrivLevel);
    fn set_to_user() { Self::set_privilege(PrivLevel::User); }
}

/// 可读可写的特权级视图（组合 trait，便于对 trap 帧等类型做统一约束）。
pub trait PrivLevelFrameView: PrivLevelWrite + PrivLevelRead {}
