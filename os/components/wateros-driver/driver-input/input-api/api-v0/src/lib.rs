//! 键盘、鼠标和平板设备的原始事件 API 与全局注册表。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 输入设备的主要用途，用于选择默认事件解释策略。
pub enum InputDeviceKind {
    /// 键盘等离散按键设备。
    Keyboard,
    /// 鼠标/触控板等指针设备。
    Pointer,
    /// 未识别用途，调用方不得假设轴语义。
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 绝对轴的闭区间硬件取值范围。
pub struct AbsoluteAxis {
    /// 轴最小值（包含）。
    pub minimum : i32,
    /// 轴最大值（包含），应不小于 `minimum`。
    pub maximum : i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 初始化时固定的输入设备元数据。
pub struct InputDeviceInfo {
    /// 固定设备名称。
    pub name : String,
    /// 设备用途类别。
    pub kind : InputDeviceKind,
    /// 可选绝对 X 轴范围。
    pub absolute_x : Option<AbsoluteAxis>,
    /// 可选绝对 Y 轴范围。
    pub absolute_y : Option<AbsoluteAxis>,
}

/// 与 Linux evdev/virtio-input 兼容的三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Linux evdev/virtio-input 兼容事件三元组。
pub struct RawInputEvent {
    /// evdev 事件类型。
    pub event_type : u16,
    /// 事件代码。
    pub code : u16,
    /// 事件值；按类型解释，可为负数。
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

static INPUT_DEVICES : Mutex<Vec<SharedInputDevice>> = Mutex::new(Vec::new());

/// 注册设备并返回稳定的全局索引。
pub fn register_input_device(device : SharedInputDevice) -> usize {
    let mut devices = INPUT_DEVICES.lock();
    devices.push(device);
    devices.len() - 1
}

/// 已注册设备数量。
pub fn input_device_count() -> usize { INPUT_DEVICES.lock().len() }

/// 按注册索引获取共享设备句柄。
pub fn input_device_at(index : usize) -> Option<SharedInputDevice> {
    INPUT_DEVICES.lock().get(index).cloned()
}

/// 获取当前注册表快照；不长期持有注册表锁。
pub fn input_devices() -> Vec<SharedInputDevice> { INPUT_DEVICES.lock().clone() }
