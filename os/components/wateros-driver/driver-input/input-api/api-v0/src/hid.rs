//! USB HID boot-report decoding into the shared evdev-compatible event model.
//!
//! USB controller transfers, interrupt scheduling and cache maintenance remain
//! platform responsibilities.  These functions only decode already-owned
//! reports and therefore can be tested without hardware.

use alloc::vec::Vec;

use crate::RawInputEvent;

pub const EV_KEY : u16 = 1;
pub const EV_REL : u16 = 2;
pub const BTN_LEFT : u16 = 0x110;
pub const BTN_RIGHT : u16 = 0x111;
pub const BTN_MIDDLE : u16 = 0x112;
pub const REL_X : u16 = 0;
pub const REL_Y : u16 = 1;
pub const REL_WHEEL : u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidDecodeError {
    ShortReport,
    InvalidLength,
    KeyRollover,
    DuplicateKey,
    UnsupportedUsage,
    UnsupportedButtons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardState {
    pub modifiers : u8,
    pub keys : [u8; 6],
}

fn key_code(usage : u8) -> Option<u16> {
    match usage {
        0x04..=0x1d => Some(30 + u16::from(usage - 0x04)), // A..Z
        0x1e..=0x27 => Some([2, 3, 4, 5, 6, 7, 8, 9, 10, 11][usize::from(usage - 0x1e)]),
        0x28 => Some(28), // Enter
        0x29 => Some(1),  // Escape
        0x2a => Some(14), // Backspace
        0x2b => Some(15), // Tab
        0x2c => Some(57), // Space
        _ => None,
    }
}

fn modifier_code(bit : u8) -> u16 {
    match bit {
        0 => 29,  // left control
        1 => 42,  // left shift
        2 => 56,  // left alt
        3 => 125, // left meta
        4 => 97,  // right control
        5 => 54,  // right shift
        6 => 100, // right alt
        _ => 126, // right meta
    }
}

fn push_key(events : &mut Vec<RawInputEvent>, code : u16, value : i32) {
    events.push(RawInputEvent { event_type : EV_KEY, code, value });
}

/// Decode an 8-byte USB HID boot keyboard report.
///
/// `previous` contains the prior modifier and six usage slots. The returned
/// state must be retained by the caller for subsequent release detection.
pub fn decode_boot_keyboard(report : &[u8], previous : KeyboardState)
                              -> Result<(Vec<RawInputEvent>, KeyboardState), HidDecodeError> {
    if report.len() < 8 {
        return Err(HidDecodeError::ShortReport);
    }
    let current : [u8; 6] = report[2..8].try_into().map_err(|_| HidDecodeError::InvalidLength)?;
    for &usage in &current {
        if (1..=3).contains(&usage) {
            return Err(HidDecodeError::KeyRollover);
        }
        if usage != 0 && key_code(usage).is_none() {
            return Err(HidDecodeError::UnsupportedUsage);
        }
    }
    for (index, &usage) in current.iter().enumerate() {
        if usage != 0 && current[..index].contains(&usage) {
            return Err(HidDecodeError::DuplicateKey);
        }
    }
    let mut events = Vec::new();
    for bit in 0..8 {
        let old = (previous.modifiers & (1 << bit)) != 0;
        let new = (report[0] & (1 << bit)) != 0;
        if old != new {
            push_key(&mut events, modifier_code(bit), i32::from(new));
        }
    }
    for &usage in &previous.keys {
        if usage != 0 && !current.contains(&usage) {
            push_key(&mut events, key_code(usage).ok_or(HidDecodeError::UnsupportedUsage)?, 0);
        }
    }
    for &usage in &current {
        if usage != 0 && !previous.keys.contains(&usage) {
            push_key(&mut events, key_code(usage).ok_or(HidDecodeError::UnsupportedUsage)?, 1);
        }
    }
    Ok((events, KeyboardState { modifiers : report[0], keys : current }))
}

/// Decode a USB HID boot mouse report (3 bytes, or 4 with vertical wheel).
pub fn decode_boot_mouse(report : &[u8], previous_buttons : u8)
                           -> Result<(Vec<RawInputEvent>, u8), HidDecodeError> {
    if !(report.len() == 3 || report.len() == 4) {
        return Err(HidDecodeError::InvalidLength);
    }
    let buttons = report[0];
    if buttons & !0x07 != 0 {
        return Err(HidDecodeError::UnsupportedButtons);
    }
    let mut events = Vec::new();
    for (mask, code) in [(1, BTN_LEFT), (2, BTN_RIGHT), (4, BTN_MIDDLE)] {
        if (buttons & mask) != (previous_buttons & mask) {
            push_key(&mut events, code, i32::from(buttons & mask != 0));
        }
    }
    let dx = i32::from(i8::from_ne_bytes([report[1]]));
    let dy = i32::from(i8::from_ne_bytes([report[2]]));
    if dx != 0 { events.push(RawInputEvent { event_type : EV_REL, code : REL_X, value : dx }); }
    if dy != 0 { events.push(RawInputEvent { event_type : EV_REL, code : REL_Y, value : dy }); }
    if report.len() == 4 {
        let wheel = i32::from(i8::from_ne_bytes([report[3]]));
        if wheel != 0 {
            events.push(RawInputEvent { event_type : EV_REL, code : REL_WHEEL, value : wheel });
        }
    }
    Ok((events, buttons))
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use super::*;

    #[test]
    fn keyboard_reports_press_and_release() {
        let (pressed, state) = decode_boot_keyboard(&[0, 0, 0x04, 0, 0, 0, 0, 0], KeyboardState::default()).unwrap();
        assert_eq!(pressed, vec![RawInputEvent { event_type : EV_KEY, code : 30, value : 1 }]);
        let (released, next) = decode_boot_keyboard(&[0; 8], state).unwrap();
        assert_eq!(released, vec![RawInputEvent { event_type : EV_KEY, code : 30, value : 0 }]);
        assert_eq!(next, KeyboardState::default());
    }

    #[test]
    fn keyboard_modifier_and_rollover_are_safe() {
        let (events, _) = decode_boot_keyboard(&[0x02, 0, 0x05, 0, 0, 0, 0, 0], KeyboardState::default()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(decode_boot_keyboard(&[0, 0, 1, 0, 0, 0, 0, 0], KeyboardState::default()), Err(HidDecodeError::KeyRollover));
    }

    #[test]
    fn keyboard_releases_modifier_and_key_from_previous_state() {
        let previous = KeyboardState { modifiers : 0x02, keys : [0x04, 0, 0, 0, 0, 0] };
        let (events, current) = decode_boot_keyboard(&[0; 8], previous).unwrap();
        assert_eq!(events, vec![RawInputEvent { event_type : EV_KEY, code : 42, value : 0 },
                                RawInputEvent { event_type : EV_KEY, code : 30, value : 0 }]);
        assert_eq!(current, KeyboardState::default());
    }

    #[test]
    fn mouse_decodes_buttons_motion_and_wheel() {
        let (events, buttons) = decode_boot_mouse(&[1, 0xfe, 3, 0xff], 0).unwrap();
        assert_eq!(buttons, 1);
        assert_eq!(events, vec![RawInputEvent { event_type : EV_KEY, code : BTN_LEFT, value : 1 },
                                RawInputEvent { event_type : EV_REL, code : REL_X, value : -2 },
                                RawInputEvent { event_type : EV_REL, code : REL_Y, value : 3 },
                                RawInputEvent { event_type : EV_REL, code : REL_WHEEL, value : -1 }]);
    }

    #[test]
    fn malformed_mouse_reports_are_rejected() {
        assert_eq!(decode_boot_mouse(&[0, 0], 0), Err(HidDecodeError::InvalidLength));
        assert_eq!(decode_boot_mouse(&[8, 0, 0], 0), Err(HidDecodeError::UnsupportedButtons));
    }
}
