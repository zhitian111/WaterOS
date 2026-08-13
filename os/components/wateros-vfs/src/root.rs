//! 单根只读访问 facade。

use super::active_impl;
use super::api::SingleRootReadView;

/// 返回当前活动后端的单根只读视图。
pub fn read_view() -> &'static impl SingleRootReadView {
    active_impl::backend()
}
