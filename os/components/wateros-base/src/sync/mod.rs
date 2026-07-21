//! 内核全局状态使用的单核、多核和一次初始化容器。

pub mod multiprocessor;
mod once;
pub mod uniprocessor;

pub use multiprocessor::MultiprocessorSafeCell;
pub use once::{BootOnceCell, OnceInitError, RuntimeOnceCell};
/// 单核环境下通过运行时借用提供独占访问的全局容器，见 [`uniprocessor::UniprocessorSafeCell`]。
pub use uniprocessor::UniprocessorSafeCell;
