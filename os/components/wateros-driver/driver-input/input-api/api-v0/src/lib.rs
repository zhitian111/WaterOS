//! 键盘、鼠标和平板设备的原始事件 API 与全局注册表。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use character_api::{CharacterDevice, CharacterDeviceKind, CharacterReadFinish,
                    CharacterReadReservation, SharedCharacterDevice};
use spin::Mutex;

pub mod hid;

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

const SUBSCRIBER_QUEUE_CAPACITY : usize = 256;

struct SubscriberState { queue : VecDeque<RawInputEvent>, dropped : u64 }
struct RegisteredInputDevice {
    device : SharedInputDevice,
    subscribers : Vec<Option<SubscriberState>>,
}
static INPUT_DEVICES : Mutex<Vec<Option<RegisteredInputDevice>>> = Mutex::new(Vec::new());

/// 注册设备并返回稳定的全局索引。
pub fn register_input_device(device : SharedInputDevice) -> usize {
    let mut devices = INPUT_DEVICES.lock();
    let index = devices.len();
    devices.push(Some(RegisteredInputDevice { device, subscribers : Vec::new() }));
    drop(devices);
    driver_api::notify_device_topology_changed();
    index
}

/// 已注册设备数量。
pub fn input_device_count() -> usize { INPUT_DEVICES.lock().iter().flatten().count() }

/// 按注册索引获取共享设备句柄。
pub fn input_device_at(index : usize) -> Option<SharedInputDevice> {
    INPUT_DEVICES.lock().get(index).and_then(Option::as_ref).map(|entry| entry.device.clone())
}

/// 获取当前注册表快照；不长期持有注册表锁。
pub fn input_devices() -> Vec<SharedInputDevice> {
    INPUT_DEVICES.lock().iter().flatten().map(|entry| entry.device.clone()).collect()
}

/// 获取稳定 slot ID 与设备句柄快照，供需要处理注销的消费者使用。
pub fn input_devices_snapshot() -> Vec<(usize, SharedInputDevice)> {
    INPUT_DEVICES.lock()
                 .iter()
                 .enumerate()
                 .filter_map(|(index, device)| {
                     device.as_ref().map(|entry| (index, entry.device.clone()))
                 })
                 .collect()
}

/// 独立、有界的输入消费者；每个硬件事件都会扇出到所有订阅者。
pub struct InputSubscription { device_index : usize, subscriber_index : usize }

pub fn subscribe_input_device(device_index : usize) -> DriverResult<InputSubscription> {
    let mut devices = INPUT_DEVICES.lock();
    let entry = devices.get_mut(device_index).and_then(Option::as_mut)
                       .ok_or(DriverError::InvalidParam)?;
    let subscriber_index = entry.subscribers.len();
    entry.subscribers.push(Some(SubscriberState { queue : VecDeque::new(), dropped : 0 }));
    Ok(InputSubscription { device_index, subscriber_index })
}

impl InputSubscription {
    fn with_state<T>(&self, f : impl FnOnce(&mut SubscriberState) -> T) -> DriverResult<T> {
        let mut devices = INPUT_DEVICES.lock();
        let state = devices.get_mut(self.device_index).and_then(Option::as_mut)
                           .and_then(|entry| entry.subscribers.get_mut(self.subscriber_index))
                           .and_then(Option::as_mut).ok_or(DriverError::InvalidParam)?;
        Ok(f(state))
    }

    fn pump_once(&self) -> DriverResult<bool> {
        let device = {
            let devices = INPUT_DEVICES.lock();
            devices.get(self.device_index).and_then(Option::as_ref)
                   .map(|entry| entry.device.clone()).ok_or(DriverError::InvalidParam)?
        };
        let Some(event) = device.lock().pop_event()? else { return Ok(false) };
        let mut devices = INPUT_DEVICES.lock();
        let entry = devices.get_mut(self.device_index).and_then(Option::as_mut)
                           .ok_or(DriverError::InvalidParam)?;
        for state in entry.subscribers.iter_mut().flatten() {
            if state.queue.len() == SUBSCRIBER_QUEUE_CAPACITY {
                state.queue.pop_front();
                state.dropped = state.dropped.saturating_add(1);
            }
            state.queue.push_back(event);
        }
        Ok(true)
    }

    pub fn pop_event(&mut self) -> DriverResult<Option<RawInputEvent>> {
        if let Some(event) = self.with_state(|state| state.queue.pop_front())? {
            return Ok(Some(event));
        }
        if !self.pump_once()? { return Ok(None) }
        self.with_state(|state| state.queue.pop_front())
    }

    pub fn has_event(&mut self) -> DriverResult<bool> {
        if self.with_state(|state| !state.queue.is_empty())? { return Ok(true) }
        self.pump_once()?;
        self.with_state(|state| !state.queue.is_empty())
    }

    pub fn dropped_events(&self) -> DriverResult<u64> {
        self.with_state(|state| state.dropped)
    }

    fn restore_front(&mut self, events : &[RawInputEvent]) -> DriverResult<()> {
        self.with_state(|state| for &event in events.iter().rev() {
            if state.queue.len() == SUBSCRIBER_QUEUE_CAPACITY {
                state.queue.pop_back();
                state.dropped = state.dropped.saturating_add(1);
            }
            state.queue.push_front(event);
        })
    }
}

impl Drop for InputSubscription {
    fn drop(&mut self) {
        let mut devices = INPUT_DEVICES.lock();
        if let Some(Some(entry)) = devices.get_mut(self.device_index) {
            if let Some(slot) = entry.subscribers.get_mut(self.subscriber_index) { *slot = None; }
        }
    }
}

pub const INPUT_EVENT_RECORD_SIZE : usize = 24;
struct ActiveEvdevRead { id : u64, events : Vec<RawInputEvent> }
struct EvdevCharacterDevice {
    input_index : usize,
    subscription : InputSubscription,
    active : Option<ActiveEvdevRead>,
    next_id : u64,
}

fn encode_event(event : RawInputEvent, output : &mut Vec<u8>) {
    // 真机单调时钟接入前，时间戳明确置零。
    output.extend_from_slice(&0i64.to_le_bytes());
    output.extend_from_slice(&0i64.to_le_bytes());
    output.extend_from_slice(&event.event_type.to_le_bytes());
    output.extend_from_slice(&event.code.to_le_bytes());
    output.extend_from_slice(&event.value.to_le_bytes());
}

impl EvdevCharacterDevice {
    fn stage(&mut self, max_len : usize) -> DriverResult<Option<(Vec<RawInputEvent>, Vec<u8>)>> {
        if max_len < INPUT_EVENT_RECORD_SIZE { return Err(DriverError::InvalidParam) }
        let mut events = Vec::new();
        while events.len() < max_len / INPUT_EVENT_RECORD_SIZE {
            let Some(event) = self.subscription.pop_event()? else { break };
            events.push(event);
        }
        if events.is_empty() { return Ok(None) }
        let mut bytes = Vec::with_capacity(events.len() * INPUT_EVENT_RECORD_SIZE);
        for &event in &events { encode_event(event, &mut bytes); }
        Ok(Some((events, bytes)))
    }
}

impl CharacterDevice for EvdevCharacterDevice {
    fn prepare_read(&mut self, max_len : usize) -> DriverResult<Option<CharacterReadReservation>> {
        if self.active.is_some() { return Ok(None) }
        let Some((events, bytes)) = self.stage(max_len)? else { return Ok(None) };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.active = Some(ActiveEvdevRead { id, events });
        Ok(Some(CharacterReadReservation::new(id, bytes)))
    }

    fn finish_read(&mut self, reservation : CharacterReadReservation, copied : usize,
                   complete : bool) -> DriverResult<CharacterReadFinish> {
        let (id, bytes) = reservation.into_parts();
        let Some(active) = self.active.take() else { return Err(DriverError::InvalidParam) };
        if active.id != id || copied > bytes.len() {
            self.subscription.restore_front(&active.events)?;
            return Err(DriverError::InvalidParam);
        }
        let consumed = copied.saturating_add(INPUT_EVENT_RECORD_SIZE - 1) / INPUT_EVENT_RECORD_SIZE;
        self.subscription.restore_front(&active.events[consumed.min(active.events.len())..])?;
        if copied == 0 && !complete { Ok(CharacterReadFinish::Fault) }
        else { Ok(CharacterReadFinish::Bytes(copied)) }
    }

    fn read(&mut self, buf : &mut [u8]) -> DriverResult<usize> {
        let Some(reservation) = self.prepare_read(buf.len())? else {
            return Err(DriverError::Unsupported);
        };
        let len = reservation.bytes().len();
        buf[..len].copy_from_slice(reservation.bytes());
        match self.finish_read(reservation, len, true)? {
            CharacterReadFinish::Bytes(copied) => Ok(copied),
            CharacterReadFinish::Fault => Err(DriverError::IoError),
        }
    }

    fn write(&mut self, _buf : &[u8]) -> DriverResult<usize> { Err(DriverError::Unsupported) }
    fn poll_revents(&mut self, events : i16) -> DriverResult<i16> {
        const POLLIN : i16 = 0x001;
        Ok(if events & POLLIN != 0 && self.active.is_none() && self.subscription.has_event()? {
            POLLIN
        } else { 0 })
    }
    fn device_kind(&self) -> CharacterDeviceKind {
        CharacterDeviceKind::InputEvent { input_index : self.input_index }
    }
}

pub fn evdev_character_device(input_index : usize) -> DriverResult<SharedCharacterDevice> {
    Ok(Arc::new(Mutex::new(Box::new(EvdevCharacterDevice {
        input_index, subscription : subscribe_input_device(input_index)?, active : None, next_id : 1,
    }))))
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

#[cfg(test)]
mod tests {
    use super::*;
    struct TestInput { info : InputDeviceInfo, events : VecDeque<RawInputEvent> }
    impl InputDevice for TestInput {
        fn info(&self) -> &InputDeviceInfo { &self.info }
        fn pop_event(&mut self) -> DriverResult<Option<RawInputEvent>> { Ok(self.events.pop_front()) }
    }
    fn register_test(events : &[RawInputEvent]) -> usize {
        register_input_device(Arc::new(Mutex::new(Box::new(TestInput {
            info : InputDeviceInfo { name : String::from("test-input"),
                                     kind : InputDeviceKind::Keyboard,
                                     absolute_x : None, absolute_y : None },
            events : events.iter().copied().collect(),
        }))))
    }
    #[test]
    fn subscriptions_receive_the_same_hardware_events() {
        let event = RawInputEvent { event_type : 1, code : 30, value : 1 };
        let index = register_test(&[event]);
        let mut gui = subscribe_input_device(index).unwrap();
        let mut evdev = subscribe_input_device(index).unwrap();
        assert_eq!(gui.pop_event().unwrap(), Some(event));
        assert_eq!(evdev.pop_event().unwrap(), Some(event));
        assert_eq!(evdev.dropped_events().unwrap(), 0);
        assert!(unregister_input_device(index));
        assert!(gui.pop_event().is_err());
    }
    #[test]
    fn evdev_layout_and_transactional_suffix_rollback() {
        let first = RawInputEvent { event_type : 1, code : 30, value : 1 };
        let second = RawInputEvent { event_type : 0, code : 0, value : 0 };
        let index = register_test(&[first, second]);
        let device = evdev_character_device(index).unwrap();
        let mut device = device.lock();
        let reservation = device.prepare_read(48).unwrap().unwrap();
        assert_eq!(reservation.bytes().len(), 48);
        assert_eq!(&reservation.bytes()[0..16], &[0; 16]);
        assert_eq!(&reservation.bytes()[16..18], &first.event_type.to_le_bytes());
        assert_eq!(&reservation.bytes()[18..20], &first.code.to_le_bytes());
        assert_eq!(&reservation.bytes()[20..24], &first.value.to_le_bytes());
        assert_eq!(device.finish_read(reservation, 24, false).unwrap(),
                   CharacterReadFinish::Bytes(24));
        let replay = device.prepare_read(24).unwrap().unwrap();
        assert_eq!(&replay.bytes()[16..18], &second.event_type.to_le_bytes());
        device.finish_read(replay, 24, true).unwrap();
        drop(device);
        assert!(unregister_input_device(index));
    }
}
