//! 单核或单线程假设下的同步原语与共享容器。

pub mod uniprocessor;

/// 单核环境下通过运行时借用提供独占访问的全局容器，见 [`uniprocessor::UniprocessorSafeCell`]。
pub use uniprocessor::UniprocessorSafeCell;

