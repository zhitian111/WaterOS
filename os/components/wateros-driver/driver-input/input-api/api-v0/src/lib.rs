//! 键盘、鼠标和平板设备的原始事件 API 与全局注册表。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 输入设备的主要用途，用于选择默认事件解释策略。
pub enum InputDeviceKind {
    Keyboard,
    Pointer,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 绝对轴的闭区间硬件取值范围。
pub struct AbsoluteAxis {
    pub minimum : i32,
    pub maximum : i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 初始化时固定的输入设备元数据。
pub struct InputDeviceInfo {
    pub name : String,
    pub kind : InputDeviceKind,
    pub absolute_x : Option<AbsoluteAxis>,
    pub absolute_y : Option<AbsoluteAxis>,
}

/// 与 Linux evdev/virtio-input 兼容的三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Linux evdev/virtio-input 兼容事件三元组。
pub struct RawInputEvent {
    pub event_type : u16,
    pub code : u16,
    pub value : i32,
}

/// 可注册输入设备。实现必须保证 [`InputDevice::pop_event`] 非阻塞。
pub trait InputDevice: Send {
    /// 返回在设备生命周期内保持稳定的元数据。
    fn info(&self) -> &InputDeviceInfo;
    /// 非阻塞取出一个事件；当前无数据时返回 `Ok(None)`。
    fn pop_event(&mut self) -> DriverResult<Option<RawInputEvent>>;
}

pub type SharedInputDevice = Arc<Mutex<Box<dyn InputDevice>>>;

static INPUT_DEVICES : Mutex<Vec<Option<SharedInputDevice>>> = Mutex::new(Vec::new());

/// 注册设备并返回稳定的全局索引。
pub fn register_input_device(device : SharedInputDevice) -> usize {
    let mut devices = INPUT_DEVICES.lock();
    let index = devices.len();
    devices.push(Some(device));
    drop(devices);
    driver_api::notify_device_topology_changed();
    index
}

/// 已注册设备数量。
pub fn input_device_count() -> usize { INPUT_DEVICES.lock().iter().flatten().count() }

/// 按注册索引获取共享设备句柄。
pub fn input_device_at(index : usize) -> Option<SharedInputDevice> {
    INPUT_DEVICES.lock().get(index).and_then(Option::as_ref).cloned()
}

/// 获取当前注册表快照；不长期持有注册表锁。
pub fn input_devices() -> Vec<SharedInputDevice> {
    INPUT_DEVICES.lock().iter().flatten().cloned().collect()
}

/// 获取稳定 slot ID 与设备句柄快照，供需要处理注销的消费者使用。
pub fn input_devices_snapshot() -> Vec<(usize, SharedInputDevice)> {
    INPUT_DEVICES.lock()
                 .iter()
                 .enumerate()
                 .filter_map(|(index, device)| {
                     device.as_ref().map(|device| (index, device.clone()))
                 })
                 .collect()
}

/// 注销输入设备；已取得的共享句柄在引用释放前仍然有效。
///
/// 真机驱动必须先屏蔽中断并停止 DMA；本 API 只处理注册表可见性，该硬件顺序待上板验证。
pub fn unregister_input_device(index : usize) -> bool {
    let mut devices = INPUT_DEVICES.lock();
    let Some(slot) = devices.get_mut(index) else { return false };
    if slot.take().is_none() {
        return false;
    }
    drop(devices);
    driver_api::notify_device_topology_changed();
    true
}
