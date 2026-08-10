//! USB HID boot-protocol report decoders.
//!
//! This module intentionally stops at the evdev-compatible raw event layer.
//! A USB host controller still has to transport reports, and its IRQ/DMA/cache
//! ordering is `UNVERIFIED_ON_HARDWARE` until a target board is available.

use crate::{DriverError, DriverResult, RawInputEvent};

pub const EV_SYN : u16 = 0;
pub const EV_KEY : u16 = 1;
pub const EV_REL : u16 = 2;
pub const SYN_REPORT : u16 = 0;
pub const REL_X : u16 = 0;
pub const REL_Y : u16 = 1;
pub const REL_WHEEL : u16 = 8;
pub const BTN_LEFT : u16 = 0x110;
pub const BTN_RIGHT : u16 = 0x111;
pub const BTN_MIDDLE : u16 = 0x112;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BootKeyboardState {
    pub modifiers : u8,
    pub keys : [u8; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BootMouseState {
    pub buttons : u8,
}

fn push_event(output : &mut [RawInputEvent], count : &mut usize, event : RawInputEvent)
                  -> DriverResult<()> {
    let slot = output.get_mut(*count).ok_or(DriverError::InvalidParam)?;
    *slot = event;
    *count += 1;
    Ok(())
}

fn key_event(code : u16, value : i32) -> RawInputEvent {
    RawInputEvent { event_type : EV_KEY, code, value }
}

/// Decode an 8-byte USB HID boot keyboard report.
///
/// The returned state must be passed to the next call. HID rollover (`0x01`)
/// is rejected instead of synthesizing releases, preventing a transient USB
/// fault from turning into stuck keys.
pub fn decode_boot_keyboard_report(state : BootKeyboardState,
                                   report : &[u8],
                                   output : &mut [RawInputEvent])
                                   -> DriverResult<(BootKeyboardState, usize)> {
    if report.len() != 8 { return Err(DriverError::InvalidParam) }
    let modifiers = report[0];
    let mut keys = [0u8; 6];
    keys.copy_from_slice(&report[2..8]);
    if keys.iter().any(|key| *key == 1) { return Err(DriverError::IoError) }

    let mut count = 0;
    for bit in 0u8..8 {
        if ((state.modifiers ^ modifiers) >> bit) & 1 != 0 {
            push_event(output, &mut count, key_event(modifier_code(bit),
                                                     if (modifiers >> bit) & 1 != 0 { 1 } else { 0 }))?;
        }
    }
    for key in state.keys {
        if key != 0 && !keys.contains(&key) {
            push_event(output, &mut count, key_event(hid_usage_to_evdev(key)
                                                      .ok_or(DriverError::InvalidParam)?, 0))?;
        }
    }
    for key in keys {
        if key != 0 && !state.keys.contains(&key) {
            push_event(output, &mut count, key_event(hid_usage_to_evdev(key)
                                                      .ok_or(DriverError::InvalidParam)?, 1))?;
        }
    }
    Ok((BootKeyboardState { modifiers, keys }, count))
}

fn modifier_code(bit : u8) -> u16 {
    match bit {
        0 => 29, 1 => 42, 2 => 56, 3 => 125,
        4 => 97, 5 => 54, 6 => 100, _ => 126,
    }
}

/// Translate the USB HID keyboard usage subset used by the boot protocol to
/// Linux evdev key codes consumed by the GUI bridge.
pub fn hid_usage_to_evdev(usage : u8) -> Option<u16> {
    Some(match usage {
        0x04..=0x1d => 30 + u16::from(usage - 0x04),
        0x1e..=0x27 => 2 + u16::from(usage - 0x1e),
        0x28 => 28, 0x29 => 1, 0x2a => 14, 0x2b => 15, 0x2c => 57,
        0x2d => 12, 0x2e => 13, 0x2f => 26, 0x30 => 27, 0x31 => 43,
        0x33 => 39, 0x34 => 40, 0x35 => 41, 0x36 => 51, 0x37 => 52,
        0x38 => 53, 0x39 => 58, 0x3a..=0x45 => 59 + u16::from(usage - 0x3a),
        _ => return None,
    })
}

/// Decode a 3-byte USB HID boot mouse report, or a 4-byte report with wheel.
pub fn decode_boot_mouse_report(state : BootMouseState,
                                report : &[u8],
                                output : &mut [RawInputEvent])
                                -> DriverResult<(BootMouseState, usize)> {
    if !(report.len() == 3 || report.len() == 4) { return Err(DriverError::InvalidParam) }
    let buttons = report[0] & 0x07;
    let mut count = 0;
    for bit in 0u8..3 {
        if ((state.buttons ^ buttons) >> bit) & 1 != 0 {
            push_event(output, &mut count, key_event(BTN_LEFT + u16::from(bit),
                                                     if (buttons >> bit) & 1 != 0 { 1 } else { 0 }))?;
        }
    }
    let x = i8::from_ne_bytes([report[1]]) as i32;
    let y = i8::from_ne_bytes([report[2]]) as i32;
    if x != 0 { push_event(output, &mut count, RawInputEvent { event_type : EV_REL, code : REL_X, value : x })?; }
    if y != 0 { push_event(output, &mut count, RawInputEvent { event_type : EV_REL, code : REL_Y, value : y })?; }
    if report.len() == 4 {
        let wheel = i8::from_ne_bytes([report[3]]) as i32;
        if wheel != 0 { push_event(output, &mut count, RawInputEvent { event_type : EV_REL, code : REL_WHEEL, value : wheel })?; }
    }
    if count != 0 {
        push_event(output, &mut count, RawInputEvent { event_type : EV_SYN, code : SYN_REPORT, value : 0 })?;
    }
    Ok((BootMouseState { buttons }, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_reports_only_state_changes() {
        let mut events = [RawInputEvent::default(); 8];
        let (state, count) = decode_boot_keyboard_report(BootKeyboardState::default(),
                                                         &[0, 0, 4, 0, 0, 0, 0, 0],
                                                         &mut events).unwrap();
        assert_eq!(count, 1);
        assert_eq!(events[0], key_event(30, 1));
        let (_, count) = decode_boot_keyboard_report(state, &[0, 0, 0, 0, 0, 0, 0, 0],
                                                      &mut events).unwrap();
        assert_eq!(count, 1);
        assert_eq!(events[0], key_event(30, 0));
    }

    #[test]
    fn keyboard_rollover_and_short_output_fail_closed() {
        let mut events = [RawInputEvent::default(); 1];
        assert_eq!(decode_boot_keyboard_report(BootKeyboardState::default(),
                                                &[0, 0, 1, 0, 0, 0, 0, 0],
                                                &mut events),
                   Err(DriverError::IoError));
        assert_eq!(decode_boot_keyboard_report(BootKeyboardState::default(),
                                                &[0; 8], &mut []).unwrap().1, 0);
    }

    #[test]
    fn mouse_emits_buttons_axes_wheel_and_sync() {
        let mut events = [RawInputEvent::default(); 8];
        let (_, count) = decode_boot_mouse_report(BootMouseState::default(),
                                                  &[1, 0xfe, 2, 0xff], &mut events).unwrap();
        assert_eq!(count, 5);
        assert_eq!(events[0], key_event(BTN_LEFT, 1));
        assert_eq!(events[1].value, -2);
        assert_eq!(events[2].value, 2);
        assert_eq!(events[3].code, REL_WHEEL);
        assert_eq!(events[4].event_type, EV_SYN);
    }

    #[test]
    fn mouse_rejects_short_report_and_small_output() {
        let mut events = [RawInputEvent::default(); 1];
        assert_eq!(decode_boot_mouse_report(BootMouseState::default(), &[0, 0], &mut events),
                   Err(DriverError::InvalidParam));
        assert_eq!(decode_boot_mouse_report(BootMouseState::default(), &[1, 0, 0], &mut events),
                   Err(DriverError::InvalidParam));
    }
}
