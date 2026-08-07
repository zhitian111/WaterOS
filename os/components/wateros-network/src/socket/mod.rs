//! WaterOS socket 对象层。
//!
//! 本模块把协议栈裸句柄封装为具有共享生命周期的 [`SocketRef`]，并将
//! 接收事务和 VFS fd 适配分别隔离为内部子模块。

mod object;
mod lease;
mod fd;

pub use lease::SocketReceiveLease;
pub use object::SocketRef;
