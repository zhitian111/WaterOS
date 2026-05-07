use crate::addr::BasePhysAddr;

/// 设备树（DTB）在物理地址空间中的基地址类型别名。
///
/// 具体数值由引导加载程序或固件填入；此处仅提供类型标记，不隐含固定物理地址。
#[allow(unused)]
pub type DTBPA = BasePhysAddr;
