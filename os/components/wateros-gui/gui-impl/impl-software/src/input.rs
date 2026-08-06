//! 原始设备事件到 GUI 事件的适配层。
//!
//! 驱动层保留 Linux evdev 编码，本层负责键盘布局、修饰键状态、指针坐标合并和
//! 屏幕坐标缩放。窗口系统因此不依赖 VirtIO，也不需要理解硬件扫描码。

use alloc::vec::Vec;
use api_v0::{
    InputEvent, KeyCode, KeyEvent, KeyModifiers, Point, PointerButton, PointerEvent,
    PointerEventKind, Size,
};
use input::{AbsoluteAxis, InputDeviceKind, RawInputEvent, SharedInputDevice};

const EV_SYN : u16 = 0;
const EV_KEY : u16 = 1;
const EV_REL : u16 = 2;
const EV_ABS : u16 = 3;
const SYN_REPORT : u16 = 0;
const REL_X : u16 = 0;
const REL_Y : u16 = 1;
const REL_HWHEEL : u16 = 6;
const REL_WHEEL : u16 = 8;
const ABS_X : u16 = 0;
const ABS_Y : u16 = 1;
const BTN_LEFT : u16 = 0x110;
const BTN_RIGHT : u16 = 0x111;
const BTN_MIDDLE : u16 = 0x112;

struct DeviceState {
    device : SharedInputDevice,
    #[allow(dead_code)]
    kind : InputDeviceKind,
    absolute_x : Option<AbsoluteAxis>,
    absolute_y : Option<AbsoluteAxis>,
    pointer : Point,
    pointer_changed : bool,
    modifiers : KeyModifiers,
}

impl DeviceState {
    fn new(device : SharedInputDevice) -> Self {
        let info = device.lock().info().clone();
        Self { device,
               kind : info.kind,
               absolute_x : info.absolute_x,
               absolute_y : info.absolute_y,
               pointer : Point::new(0, 0),
               pointer_changed : false,
               modifiers : KeyModifiers::default() }
    }

    fn consume(&mut self, raw : RawInputEvent, size : Size, output : &mut Vec<InputEvent>) {
        match raw.event_type {
            EV_SYN if raw.code == SYN_REPORT => {
                if self.pointer_changed {
                    output.push(InputEvent::Pointer(PointerEvent {
                        position : self.pointer,
                        kind : PointerEventKind::Move,
                    }));
                    self.pointer_changed = false;
                }
            }
            EV_ABS if raw.code == ABS_X => {
                self.pointer.x = scale_axis(raw.value, self.absolute_x, size.width);
                self.pointer_changed = true;
            }
            EV_ABS if raw.code == ABS_Y => {
                self.pointer.y = scale_axis(raw.value, self.absolute_y, size.height);
                self.pointer_changed = true;
            }
            EV_REL if raw.code == REL_X => {
                self.pointer.x = clamp_coordinate(self.pointer.x.saturating_add(raw.value), size.width);
                self.pointer_changed = true;
            }
            EV_REL if raw.code == REL_Y => {
                self.pointer.y = clamp_coordinate(self.pointer.y.saturating_add(raw.value), size.height);
                self.pointer_changed = true;
            }
            EV_REL if raw.code == REL_WHEEL || raw.code == REL_HWHEEL => {
                output.push(InputEvent::Pointer(PointerEvent {
                    position : self.pointer,
                    kind : PointerEventKind::Scroll {
                        horizontal : if raw.code == REL_HWHEEL { raw.value } else { 0 },
                        vertical : if raw.code == REL_WHEEL { raw.value } else { 0 },
                    },
                }));
            }
            EV_KEY if raw.code >= BTN_LEFT => {
                output.push(InputEvent::Pointer(PointerEvent {
                    position : self.pointer,
                    kind : PointerEventKind::Button {
                        button : pointer_button(raw.code),
                        pressed : raw.value != 0,
                    },
                }));
            }
            EV_KEY => self.consume_key(raw.code, raw.value, output),
            _ => {}
        }
    }

    fn consume_key(&mut self, code : u16, value : i32, output : &mut Vec<InputEvent>) {
        let pressed = value != 0;
        let repeat = value == 2;
        let is_modifier = update_modifier(&mut self.modifiers, code, pressed);
        let key = key_code(code, self.modifiers);
        output.push(InputEvent::Key(KeyEvent {
            code : key,
            modifiers : self.modifiers,
            pressed,
            repeat,
        }));
        if !is_modifier && pressed && !self.modifiers.contains(KeyModifiers::CTRL) &&
           !self.modifiers.contains(KeyModifiers::ALT)
        {
            if let Some(character) = printable_character(code, self.modifiers) {
                output.push(InputEvent::Text(character));
            }
        }
    }
}

/// 一组输入设备及其跨事件状态。设备注册表增长时会自动发现新设备。
pub struct InputBridge {
    devices : Vec<DeviceState>,
}

impl InputBridge {
    pub const fn new() -> Self { Self { devices : Vec::new() } }

    /// 至多消费 `budget` 个原始事件，防止输入轮询饿死合成和调度。
    pub fn poll(&mut self, size : Size, budget : usize) -> Vec<InputEvent> {
        self.discover_devices();
        let mut output = Vec::new();
        let mut remaining = budget;
        for state in &mut self.devices {
            while remaining > 0 {
                // 设备锁仅覆盖一次非阻塞 pop，不跨事件路由或绘制。
                let raw = state.device.lock().pop_event();
                match raw {
                    Ok(Some(raw)) => {
                        state.consume(raw, size, &mut output);
                        remaining -= 1;
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            if remaining == 0 {
                break;
            }
        }
        output
    }

    pub fn device_count(&self) -> usize { self.devices.len() }

    fn discover_devices(&mut self) {
        let count = input::input_device_count();
        for index in self.devices.len()..count {
            if let Some(device) = input::input_device_at(index) {
                self.devices.push(DeviceState::new(device));
            }
        }
    }
}

impl Default for InputBridge {
    fn default() -> Self { Self::new() }
}

fn scale_axis(value : i32, axis : Option<AbsoluteAxis>, extent : u32) -> i32 {
    let Some(axis) = axis else { return clamp_coordinate(value, extent) };
    if extent <= 1 || axis.maximum <= axis.minimum {
        return 0;
    }
    let value = value.clamp(axis.minimum, axis.maximum) - axis.minimum;
    let range = (axis.maximum - axis.minimum) as i64;
    ((value as i64 * (extent - 1) as i64) / range) as i32
}

fn clamp_coordinate(value : i32, extent : u32) -> i32 {
    value.clamp(0, extent.saturating_sub(1).min(i32::MAX as u32) as i32)
}

fn pointer_button(code : u16) -> PointerButton {
    match code {
        BTN_LEFT => PointerButton::Left,
        BTN_RIGHT => PointerButton::Right,
        BTN_MIDDLE => PointerButton::Middle,
        other => PointerButton::Other(other),
    }
}

fn update_modifier(modifiers : &mut KeyModifiers, code : u16, pressed : bool) -> bool {
    let flag = match code {
        42 | 54 => KeyModifiers::SHIFT,
        29 | 97 => KeyModifiers::CTRL,
        56 | 100 => KeyModifiers::ALT,
        125 | 126 => KeyModifiers::SUPER,
        58 if pressed => {
            modifiers.0 ^= KeyModifiers::CAPS_LOCK;
            return true;
        }
        58 => return true,
        _ => return false,
    };
    if pressed { modifiers.0 |= flag } else { modifiers.0 &= !flag }
    true
}

fn key_code(code : u16, modifiers : KeyModifiers) -> KeyCode {
    match code {
        1 => KeyCode::Escape,
        14 => KeyCode::Backspace,
        15 => KeyCode::Tab,
        28 | 96 => KeyCode::Enter,
        57 => KeyCode::Space,
        59..=68 => KeyCode::Function((code - 58) as u8),
        87..=88 => KeyCode::Function((code - 76) as u8),
        102 => KeyCode::Home,
        103 => KeyCode::Up,
        104 => KeyCode::PageUp,
        105 => KeyCode::Left,
        106 => KeyCode::Right,
        107 => KeyCode::End,
        108 => KeyCode::Down,
        109 => KeyCode::PageDown,
        110 => KeyCode::Insert,
        111 => KeyCode::Delete,
        _ => printable_character(code, modifiers).map_or(KeyCode::Unknown(code), KeyCode::Character),
    }
}

fn printable_character(code : u16, modifiers : KeyModifiers) -> Option<char> {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let caps = modifiers.contains(KeyModifiers::CAPS_LOCK);
    let pair = match code {
        2 => ('1', '!'), 3 => ('2', '@'), 4 => ('3', '#'), 5 => ('4', '$'),
        6 => ('5', '%'), 7 => ('6', '^'), 8 => ('7', '&'), 9 => ('8', '*'),
        10 => ('9', '('), 11 => ('0', ')'), 12 => ('-', '_'), 13 => ('=', '+'),
        16 => ('q', 'Q'), 17 => ('w', 'W'), 18 => ('e', 'E'), 19 => ('r', 'R'),
        20 => ('t', 'T'), 21 => ('y', 'Y'), 22 => ('u', 'U'), 23 => ('i', 'I'),
        24 => ('o', 'O'), 25 => ('p', 'P'), 26 => ('[', '{'), 27 => (']', '}'),
        30 => ('a', 'A'), 31 => ('s', 'S'), 32 => ('d', 'D'), 33 => ('f', 'F'),
        34 => ('g', 'G'), 35 => ('h', 'H'), 36 => ('j', 'J'), 37 => ('k', 'K'),
        38 => ('l', 'L'), 39 => (';', ':'), 40 => ('\'', '"'), 41 => ('`', '~'),
        43 => ('\\', '|'), 44 => ('z', 'Z'), 45 => ('x', 'X'), 46 => ('c', 'C'),
        47 => ('v', 'V'), 48 => ('b', 'B'), 49 => ('n', 'N'), 50 => ('m', 'M'),
        51 => (',', '<'), 52 => ('.', '>'), 53 => ('/', '?'), 57 => (' ', ' '),
        _ => return None,
    };
    let is_letter = pair.0.is_ascii_alphabetic();
    Some(if shift ^ (caps && is_letter) { pair.1 } else { pair.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_coordinates_reach_both_edges() {
        let axis = Some(AbsoluteAxis { minimum : 0, maximum : 32_767 });
        assert_eq!(scale_axis(0, axis, 800), 0);
        assert_eq!(scale_axis(32_767, axis, 800), 799);
    }

    #[test]
    fn shift_and_caps_follow_us_layout() {
        assert_eq!(printable_character(30, KeyModifiers(0)), Some('a'));
        assert_eq!(printable_character(30, KeyModifiers(KeyModifiers::SHIFT)), Some('A'));
        assert_eq!(printable_character(30, KeyModifiers(KeyModifiers::CAPS_LOCK)), Some('A'));
        assert_eq!(printable_character(2, KeyModifiers(KeyModifiers::SHIFT)), Some('!'));
    }
}
