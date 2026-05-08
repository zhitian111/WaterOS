//! 单核或单线程假设下的同步原语与共享容器。
//!
//! 多 hart 或抢占并发路径请使用平台互斥或其它专用同步子系统，勿误用此处容器。

pub mod uniprocessor;

/// 单核环境下通过运行时借用提供独占访问的全局容器，见 [`uniprocessor::UniprocessorSafeCell`]。
pub use uniprocessor::UniprocessorSafeCell;

