//! 用户态图形设备句柄：Linux fbdev 数据面与 evdev 事件流。
//!
//! Linux ioctl 的用户指针翻译仍在 syscall 层。本模块只管理打开文件
//! 描述、framebuffer 字节访问、设备 mmap 元数据和已时间戳的输入事件。

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::{
    VfsCopyProgress, VfsDeviceMapping, VfsDeviceMappingLease, VfsError,
    VfsFramebufferInfo, VfsInputDeviceInfo, VfsIoHandle, VfsMetadata, VfsNodeType,
    VfsOpenDescriptionState, VfsPreparedRead, VfsReadFinish, VfsReadLease,
    VfsReadReservation, VfsResult, VfsSeekWhence, VfsSpecialDeviceInfo,
};
use driver_display_api_v0::{FramebufferInfo, SharedDisplayDevice};
use driver_input_api_v0::{
    input_devices, InputDeviceInfo, InputDeviceKind, RawInputEvent, SharedInputDevice,
};
use spin::Mutex;

const POLLIN : i16 = 0x001;
const POLLOUT : i16 = 0x004;
const O_NONBLOCK : u32 = 0o4000;
const EVENT_BYTES : usize = 24;
const CLIENT_EVENT_CAPACITY : usize = 256;
const EV_SYN : u16 = 0;
const SYN_DROPPED : u16 = 3;

fn path_inode(path : &str) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01B3);
    }
    hash | (1u64 << 63)
}

fn special_metadata(path : &str, size : u64, mode : u16) -> VfsMetadata {
    VfsMetadata { node_type : VfsNodeType::Special,
                  size,
                  mode,
                  device_major : 0,
                  device_minor : 0x7FFE_0001,
                  inode : path_inode(path),
                  mount_id : 0,
                  nlink : 1,
                  uid : 0,
                  gid : 0 }
}

fn map_fb_info(info : FramebufferInfo) -> VfsFramebufferInfo {
    VfsFramebufferInfo { width : info.width,
                         height : info.height,
                         stride : info.stride,
                         byte_len : info.byte_len,
                         phys_base : info.phys_base,
                         mapped_len : info.mapped_len }
}

struct FramebufferPreparedRead {
    device : SharedDisplayDevice,
    description : Arc<VfsOpenDescriptionState>,
    reservation : VfsReadReservation,
    max_len : usize,
}

impl VfsPreparedRead for FramebufferPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let info = self.device.lock().info();
        let start = usize::try_from(self.reservation.offset()).map_err(|_| VfsError::Io)?;
        let len = info.byte_len.saturating_sub(start).min(self.max_len);
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| VfsError::NoMemory)?;
        bytes.resize(len, 0);
        if len != 0 {
            let mut device = self.device.lock();
            let framebuffer = device.framebuffer().map_err(|_| VfsError::Driver)?;
            bytes.copy_from_slice(&framebuffer[start..start + len]);
        }
        Ok(Box::new(FramebufferReadLease { description : self.description,
                                           reservation : Some(self.reservation),
                                           bytes }))
    }
}

struct FramebufferReadLease {
    description : Arc<VfsOpenDescriptionState>,
    reservation : Option<VfsReadReservation>,
    bytes : Vec<u8>,
}

impl VfsReadLease for FramebufferReadLease {
    fn bytes(&self) -> &[u8] { &self.bytes }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        let copied = progress.copied.min(self.bytes.len());
        self.description.finish_read(reservation, copied, self.bytes.len())?;
        Ok(if progress.complete { VfsReadFinish::Bytes(copied) } else { VfsReadFinish::Fault })
    }
}

impl Drop for FramebufferReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = self.description.finish_read(reservation, 0, self.bytes.len());
        }
    }
}

/// `/dev/fb0` 的打开文件描述。
pub struct FramebufferHandle {
    device : SharedDisplayDevice,
    description : Arc<VfsOpenDescriptionState>,
    accmode : u32,
}

impl FramebufferHandle {
    fn new(device : SharedDisplayDevice, accmode : u32, nonblocking : bool) -> Self {
        Self { device,
               description : Arc::new(VfsOpenDescriptionState::new(
                   0,
                   if nonblocking { O_NONBLOCK } else { 0 },
               )),
               accmode }
    }

    fn write_at_inner(&self, offset : usize, buf : &[u8]) -> VfsResult<usize> {
        if self.accmode == 0 {
            return Err(VfsError::BadFd);
        }
        let mut device = self.device.lock();
        let framebuffer = device.framebuffer().map_err(|_| VfsError::Driver)?;
        if offset >= framebuffer.len() {
            return Ok(0);
        }
        let len = framebuffer.len().saturating_sub(offset).min(buf.len());
        if len != 0 {
            framebuffer[offset..offset + len].copy_from_slice(&buf[..len]);
        }
        Ok(len)
    }
}

impl VfsIoHandle for FramebufferHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        if self.accmode == 1 {
            return Err(VfsError::BadFd);
        }
        let reservation = self.description.begin_read()?;
        Ok(Box::new(FramebufferPreparedRead { device : self.device.clone(),
                                              description : self.description.clone(),
                                              reservation,
                                              max_len }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        let offset = usize::try_from(self.description.offset()).map_err(|_| VfsError::Io)?;
        let mut device = self.device.lock();
        let framebuffer = device.framebuffer().map_err(|_| VfsError::Driver)?;
        if offset >= framebuffer.len() {
            return Ok(0);
        }
        let len = framebuffer.len().saturating_sub(offset).min(buf.len());
        buf[..len].copy_from_slice(&framebuffer[offset..offset + len]);
        drop(device);
        self.description.advance_offset(len as u64)?;
        Ok(len)
    }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        let offset = usize::try_from(self.description.offset()).map_err(|_| VfsError::Io)?;
        let written = self.write_at_inner(offset, buf)?;
        self.description.advance_offset(written as u64)?;
        Ok(written)
    }

    fn read_at(&mut self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        if self.accmode == 1 {
            return Err(VfsError::BadFd);
        }
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        let mut device = self.device.lock();
        let framebuffer = device.framebuffer().map_err(|_| VfsError::Driver)?;
        if start >= framebuffer.len() {
            return Ok(0);
        }
        let len = framebuffer.len().saturating_sub(start).min(buf.len());
        buf[..len].copy_from_slice(&framebuffer[start..start + len]);
        Ok(len)
    }

    fn write_at(&mut self, offset : u64, buf : &[u8]) -> VfsResult<usize> {
        self.write_at_inner(usize::try_from(offset).map_err(|_| VfsError::Io)?, buf)
    }

    fn seek(&mut self, offset : i64, whence : VfsSeekWhence) -> VfsResult<u64> {
        let len = self.device.lock().info().byte_len as u64;
        match whence {
            VfsSeekWhence::Set if offset >= 0 => self.description.set_offset_if_idle(offset as u64),
            VfsSeekWhence::Cur => self.description.add_signed_offset(offset),
            VfsSeekWhence::End => {
                let next = if offset < 0 {
                    len.checked_sub(offset.unsigned_abs())
                } else {
                    len.checked_add(offset as u64)
                }.ok_or(VfsError::InvalidPath)?;
                self.description.set_offset_if_idle(next)
            }
            _ => Err(VfsError::InvalidPath),
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_metadata("/dev/fb0", self.device.lock().info().byte_len as u64, 0o20660))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { device : self.device.clone(),
                           description : self.description.clone(),
                           accmode : self.accmode }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        let mut result = 0;
        if events & POLLIN != 0 && self.accmode != 1 { result |= POLLIN; }
        if events & POLLOUT != 0 && self.accmode != 0 { result |= POLLOUT; }
        Ok(result)
    }

    fn open_status_flags(&self) -> u32 { self.description.status_flags() }
    fn open_accmode(&self) -> u32 { self.accmode }
    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.description.set_status_flags(flags & O_NONBLOCK);
        Ok(())
    }

    fn special_device_info(&self) -> Option<VfsSpecialDeviceInfo> {
        Some(VfsSpecialDeviceInfo::Framebuffer(map_fb_info(self.device.lock().info())))
    }

    fn device_mapping(&self) -> VfsResult<VfsDeviceMapping> {
        let info = self.device.lock().info();
        let lease : Arc<dyn VfsDeviceMappingLease> = self.device.clone();
        Ok(VfsDeviceMapping { phys_start : info.phys_base,
                              len : info.mapped_len,
                              lease })
    }

    fn flush_device(&mut self) -> VfsResult<()> {
        self.device.lock().flush().map_err(|_| VfsError::Driver)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinuxInputEvent {
    sec : i64,
    usec : i64,
    event_type : u16,
    code : u16,
    value : i32,
}

const _ : () = assert!(core::mem::size_of::<LinuxInputEvent>() == EVENT_BYTES);

impl LinuxInputEvent {
    fn new(raw : RawInputEvent) -> Self {
        let millis = task::current_tick()
            .saturating_mul(base_config::task::SCHED_TIMER_PERIOD_MS as u64);
        Self { sec : (millis / 1000) as i64,
               usec : ((millis % 1000) * 1000) as i64,
               event_type : raw.event_type,
               code : raw.code,
               value : raw.value }
    }

    fn syn_dropped_like(event : Self) -> Self {
        Self { event_type : EV_SYN, code : SYN_DROPPED, value : 0, ..event }
    }

    fn append_bytes(self, output : &mut Vec<u8>) {
        output.extend_from_slice(&self.sec.to_ne_bytes());
        output.extend_from_slice(&self.usec.to_ne_bytes());
        output.extend_from_slice(&self.event_type.to_ne_bytes());
        output.extend_from_slice(&self.code.to_ne_bytes());
        output.extend_from_slice(&self.value.to_ne_bytes());
    }
}

struct EvdevClient {
    queue : VecDeque<LinuxInputEvent>,
    read_reserved : bool,
    wait : task::WaitQueue,
}

struct EvdevSlot {
    device : SharedInputDevice,
    info : InputDeviceInfo,
    clients : Vec<Weak<Mutex<EvdevClient>>>,
}

struct EvdevHub { slots : Vec<EvdevSlot> }

static EVDEV_HUB : Mutex<EvdevHub> = Mutex::new(EvdevHub { slots : Vec::new() });

fn map_input_info(info : &InputDeviceInfo) -> VfsInputDeviceInfo {
    VfsInputDeviceInfo { name : info.name.clone(),
                         keyboard : info.kind == InputDeviceKind::Keyboard,
                         pointer : info.kind == InputDeviceKind::Pointer,
                         absolute_x : info.absolute_x.map(|axis| (axis.minimum, axis.maximum)),
                         absolute_y : info.absolute_y.map(|axis| (axis.minimum, axis.maximum)) }
}

fn input_slot_for_path(path : &str) -> Option<usize> {
    let hub = EVDEV_HUB.lock();
    if let Some(suffix) = path.strip_prefix("/dev/input/event") {
        return suffix.parse::<usize>().ok().filter(|index| *index < hub.slots.len());
    }
    match path {
        "/dev/input/keyboard0" => hub.slots.iter().position(|slot| {
            slot.info.kind == InputDeviceKind::Keyboard
        }),
        "/dev/input/pointer0" => hub.slots.iter().position(|slot| {
            slot.info.kind == InputDeviceKind::Pointer
        }),
        _ => None,
    }
}

struct EvdevPreparedRead {
    client : Arc<Mutex<EvdevClient>>,
    nonblocking : Arc<AtomicBool>,
    max_len : usize,
}

impl VfsPreparedRead for EvdevPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        if self.max_len < EVENT_BYTES {
            self.client.lock().read_reserved = false;
            return Err(VfsError::InvalidPath);
        }
        loop {
            let (wait, available) = {
                let client = self.client.lock();
                (client.wait, client.queue.len())
            };
            if available != 0 {
                let event_count = available.min(self.max_len / EVENT_BYTES);
                let mut bytes = Vec::new();
                if bytes.try_reserve_exact(event_count * EVENT_BYTES).is_err() {
                    self.client.lock().read_reserved = false;
                    return Err(VfsError::NoMemory);
                }
                {
                    let client = self.client.lock();
                    for event in client.queue.iter().take(event_count) {
                        event.append_bytes(&mut bytes);
                    }
                }
                return Ok(Box::new(EvdevReadLease { client : self.client.clone(),
                                                    event_count,
                                                    bytes }));
            }
            if self.nonblocking.load(Ordering::Acquire) {
                self.client.lock().read_reserved = false;
                return Err(VfsError::WouldBlock);
            }
            match wait.wait_current_while(|| self.client.lock().queue.is_empty()) {
                task::TaskWaitResult::Interrupted => {
                    self.client.lock().read_reserved = false;
                    return Err(VfsError::Interrupted);
                }
                task::TaskWaitResult::Woken | task::TaskWaitResult::TimedOut => {}
            }
        }
    }
}

struct EvdevReadLease {
    client : Arc<Mutex<EvdevClient>>,
    event_count : usize,
    bytes : Vec<u8>,
}

impl VfsReadLease for EvdevReadLease {
    fn bytes(&self) -> &[u8] { &self.bytes }

    fn finish(self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        let copied = progress.copied.min(self.bytes.len());
        let completed_events = (copied / EVENT_BYTES).min(self.event_count);
        let mut client = self.client.lock();
        for _ in 0..completed_events { let _ = client.queue.pop_front(); }
        client.read_reserved = false;
        Ok(if progress.complete && copied % EVENT_BYTES == 0 {
            VfsReadFinish::Bytes(copied)
        } else {
            VfsReadFinish::Fault
        })
    }
}

impl Drop for EvdevReadLease {
    fn drop(&mut self) { self.client.lock().read_reserved = false; }
}

pub struct EvdevHandle {
    slot : usize,
    path : String,
    info : VfsInputDeviceInfo,
    client : Arc<Mutex<EvdevClient>>,
    nonblocking : Arc<AtomicBool>,
    accmode : u32,
}

impl EvdevHandle {
    fn open(path : &str, slot : usize, accmode : u32, nonblocking : bool) -> VfsResult<Self> {
        if accmode == 1 { return Err(VfsError::BadFd); }
        let client = Arc::new(Mutex::new(EvdevClient { queue : VecDeque::new(),
                                                       read_reserved : false,
                                                       wait : task::WaitQueue::new_named("evdev") }));
        let info = {
            let mut hub = EVDEV_HUB.lock();
            let entry = hub.slots.get_mut(slot).ok_or(VfsError::NotFound)?;
            entry.clients.push(Arc::downgrade(&client));
            map_input_info(&entry.info)
        };
        Ok(Self { slot,
                  path : path.to_string(),
                  info,
                  client,
                  nonblocking : Arc::new(AtomicBool::new(nonblocking)),
                  accmode })
    }
}

impl VfsIoHandle for EvdevHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let mut client = self.client.lock();
        if client.read_reserved { return Err(VfsError::Busy); }
        client.read_reserved = true;
        drop(client);
        Ok(Box::new(EvdevPreparedRead { client : self.client.clone(),
                                       nonblocking : self.nonblocking.clone(),
                                       max_len }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> {
        let lease = self.prepare_read(buf.len())?.acquire()?;
        let len = lease.bytes().len();
        buf[..len].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied : len, complete : true })? {
            VfsReadFinish::Bytes(bytes) => Ok(bytes),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_metadata(self.path.as_str(), 0, 0o20660))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { slot : self.slot,
                           path : self.path.clone(),
                           info : self.info.clone(),
                           client : self.client.clone(),
                           nonblocking : self.nonblocking.clone(),
                           accmode : self.accmode }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        Ok(if events & POLLIN != 0 && !self.client.lock().queue.is_empty() { POLLIN } else { 0 })
    }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        if events & POLLIN == 0 || !still_waiting() { return Ok(()); }
        let wait = self.client.lock().wait;
        let result = wait.wait_current_while_for_ticks(timeout_ticks.max(1), || {
            self.client.lock().queue.is_empty()
        });
        if result == task::TaskWaitResult::Interrupted { Err(VfsError::Interrupted) } else { Ok(()) }
    }

    fn open_status_flags(&self) -> u32 {
        if self.nonblocking.load(Ordering::Acquire) { O_NONBLOCK } else { 0 }
    }
    fn open_accmode(&self) -> u32 { self.accmode }
    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.nonblocking.store(flags & O_NONBLOCK != 0, Ordering::Release);
        Ok(())
    }
    fn special_device_info(&self) -> Option<VfsSpecialDeviceInfo> {
        Some(VfsSpecialDeviceInfo::InputEvent(self.info.clone()))
    }
}

/// 在驱动探测完成后建立稳定 evdev 索引。
pub fn initialize_user_graphics_devices() -> bool {
    let mut slots = Vec::new();
    for device in input_devices() {
        let info = device.lock().info().clone();
        slots.push(EvdevSlot { device, info, clients : Vec::new() });
    }
    let ready = !slots.is_empty();
    EVDEV_HUB.lock().slots = slots;
    ready
}

/// 将每个驱动的非阻塞原始事件广播到各打开者。
fn poll_input_once() -> bool {
    let snapshots : Vec<_> = {
        let mut hub = EVDEV_HUB.lock();
        for slot in &mut hub.slots { slot.clients.retain(|client| client.strong_count() != 0); }
        hub.slots.iter().map(|slot| {
            (slot.device.clone(), slot.clients.clone())
        }).collect()
    };
    let mut progressed = false;
    for (device, clients) in snapshots {
        for _ in 0..64 {
            let raw = match device.lock().pop_event() {
                Ok(Some(event)) => event,
                Ok(None) | Err(_) => break,
            };
            progressed = true;
            let event = LinuxInputEvent::new(raw);
            let mut wake = Vec::new();
            for weak in &clients {
                let Some(client) = weak.upgrade() else { continue; };
                let wait = {
                    let mut client = client.lock();
                    if client.queue.len() >= CLIENT_EVENT_CAPACITY {
                        client.queue.clear();
                        client.queue.push_back(LinuxInputEvent::syn_dropped_like(event));
                    }
                    client.queue.push_back(event);
                    client.wait
                };
                wake.push(wait);
            }
            for wait in wake { wait.wake_all(); }
        }
    }
    progressed
}

/// 由顶层启动一次的低优先级输入轮询任务。
pub extern "C" fn user_graphics_input_worker(_arg : usize) -> ! {
    loop {
        if poll_input_once() {
            task::yield_now();
        } else {
            let _ = task::sleep_for_ticks(1);
        }
    }
}

pub fn special_device_exists(path : &str) -> bool {
    (path == "/dev/fb0" && driver_display_api_v0::first_display_device().is_some()) ||
    input_slot_for_path(path).is_some()
}

pub fn special_device_metadata(path : &str) -> Option<VfsMetadata> {
    if path == "/dev/fb0" {
        let size = driver_display_api_v0::first_display_device()?.lock().info().byte_len as u64;
        return Some(special_metadata(path, size, 0o20660));
    }
    input_slot_for_path(path).map(|_| special_metadata(path, 0, 0o20660))
}

pub fn special_device_paths() -> Vec<String> {
    let mut paths = Vec::new();
    if driver_display_api_v0::first_display_device().is_some() {
        paths.push("/dev/fb0".to_string());
    }
    let hub = EVDEV_HUB.lock();
    for index in 0..hub.slots.len() { paths.push(alloc::format!("/dev/input/event{index}")); }
    if hub.slots.iter().any(|slot| slot.info.kind == InputDeviceKind::Keyboard) {
        paths.push("/dev/input/keyboard0".to_string());
    }
    if hub.slots.iter().any(|slot| slot.info.kind == InputDeviceKind::Pointer) {
        paths.push("/dev/input/pointer0".to_string());
    }
    paths
}

pub fn open_special_device(path : &str,
                           accmode : u32,
                           nonblocking : bool)
                           -> Option<VfsResult<Box<dyn VfsIoHandle>>> {
    if path == "/dev/fb0" {
        return driver_display_api_v0::first_display_device().map(|device| {
            Ok(Box::new(FramebufferHandle::new(device, accmode, nonblocking)) as Box<dyn VfsIoHandle>)
        });
    }
    input_slot_for_path(path).map(|slot| {
        EvdevHandle::open(path, slot, accmode, nonblocking)
            .map(|handle| Box::new(handle) as Box<dyn VfsIoHandle>)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_input_event_layout_and_encoding() {
        let event = LinuxInputEvent { sec : 1,
                                      usec : 2,
                                      event_type : 3,
                                      code : 4,
                                      value : 5 };
        let mut bytes = Vec::new();
        event.append_bytes(&mut bytes);
        assert_eq!(bytes.len(), EVENT_BYTES);
        assert_eq!(&bytes[16..18], &3u16.to_ne_bytes());
        assert_eq!(&bytes[20..24], &5i32.to_ne_bytes());
    }
}
