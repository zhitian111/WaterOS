//! 显示设备的稳定 API：描述线性帧缓冲并维护全局设备注册表。
//!
//! 这一层不关心 VirtIO、PCI 或具体像素绘制算法。驱动负责提供 BGRA
//! 帧缓冲和刷新操作；上层 Canvas 在设备锁保护下修改缓冲，再显式调用
//! [`DisplayDevice::flush`] 将内容提交给宿主显示设备。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

/// 当前显示驱动向绘制层提供的像素布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 每像素 4 字节，内存顺序依次为蓝、绿、红、透明度。
    Bgra8888,
}

/// 一块线性帧缓冲的只读元信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferInfo {
    /// 水平像素数。
    pub width : u32,
    /// 垂直像素数。
    pub height : u32,
    /// 相邻两行起点之间的字节数。
    pub stride : usize,
    /// 像素格式。
    pub format : PixelFormat,
    /// 可写缓冲区总字节数。
    pub byte_len : usize,
    /// 内核恒等映射下的帧缓冲虚拟地址，仅供诊断显示。
    pub base : usize,
}

/// framebuffer 中需要提交的矩形区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferRegion {
    pub x : u32,
    pub y : u32,
    pub width : u32,
    pub height : u32,
}

/// 可在多任务间共享的显示设备。锁同时保护驱动状态和可写 framebuffer。
pub type SharedDisplayDevice = Arc<Mutex<Box<dyn DisplayDevice>>>;

static DISPLAY_DEVICES : Mutex<Vec<Option<SharedDisplayDevice>>> = Mutex::new(Vec::new());

/// 显示设备契约。首版仅支持单个线性 framebuffer 与主动全屏刷新。
pub trait DisplayDevice: Send {
    /// 查询分辨率、步长与像素格式。
    fn info(&self) -> FramebufferInfo;

    /// 借用可写帧缓冲。返回借用的生命周期不得超过设备锁 guard。
    fn framebuffer(&mut self) -> DriverResult<&mut [u8]>;

    /// 将软件写入的 framebuffer 提交到显示设备。
    fn flush(&mut self) -> DriverResult<()>;

    /// 提交一个矩形区域。首版 VirtIO GPU 仍会退化为全屏刷新；提供此接口是为了让
    /// GUI 的脏矩形策略不依赖具体设备，后续驱动可直接覆盖为区域传输。
    fn flush_region(&mut self, _region : FramebufferRegion) -> DriverResult<()> { self.flush() }
}

/// 注册一个显示设备并返回稳定索引。
pub fn register_display_device(device : SharedDisplayDevice) -> usize {
    let mut devices = DISPLAY_DEVICES.lock();
    let index = devices.len();
    devices.push(Some(device));
    drop(devices);
    driver_api::notify_device_topology_changed();
    index
}

/// 返回已经注册的显示设备数量。
pub fn display_device_count() -> usize {
    DISPLAY_DEVICES.lock().iter().flatten().count()
}

/// 返回第一个显示设备；未发现 GPU 时为 `None`。
pub fn first_display_device() -> Option<SharedDisplayDevice> {
    DISPLAY_DEVICES.lock().iter().flatten().next().cloned()
}

/// 按注册索引查询显示设备。
pub fn display_device_at(index : usize) -> Option<SharedDisplayDevice> {
    DISPLAY_DEVICES.lock().get(index).and_then(Option::as_ref).cloned()
}

/// 注销显示设备；GUI 必须先停止呈现并释放 framebuffer 借用。
///
/// QEMU 可验证注册表状态，真实 GPU/显示控制器的 DMA quiesce 仍需上板测试。
pub fn unregister_display_device(index : usize) -> bool {
    let mut devices = DISPLAY_DEVICES.lock();
    let Some(slot) = devices.get_mut(index) else { return false };
    if slot.take().is_none() {
        return false;
    }
    drop(devices);
    driver_api::notify_device_topology_changed();
    true
}
