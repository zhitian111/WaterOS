//! Loongson 2K1000LA 启动参数：U-Boot `go` 通过 argc/argv 传递参数。
//!
//! 旧分支曾假设 UEFI ABI（a0=efi_boot/a1=cmdline/a2=EFI system table + DTB GUID），
//! vendor/legacy 固件还可能把 DTB 放在第三个寄存器；两种形式都在这里兼容。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct Loongson2K1000BootArgs;

impl PlatformBootArgs for Loongson2K1000BootArgs {}

pub use Loongson2K1000BootArgs as BootArgs;

/// 返回当前保存的 DTB 物理基址（PMON 通常不提供；为 0 时内存用板级回退）。
pub fn device_tree_phys_addr() -> usize {
    crate::dtb::dtb_pa()
}

const UHI_FDT_ARG0 : usize = usize::MAX - 1;
const MAX_UBOOT_GO_ARGS : usize = 16;
const MAX_UBOOT_GO_ARG_LEN : usize = 32;
const FDT_MAGIC : [u8; 4] = [0xd0, 0x0d, 0xfe, 0xed];
const FDT_HEADER_PREFIX_LEN : usize = 8;
const FDT_MIN_TOTAL_SIZE : usize = 40;
const LOW_BOOT_RAM_START : usize = 0x0000_1000;
const LOW_BOOT_RAM_END : usize = 0x0100_0000;
const RAM_START : usize = 0x9000_0000;
const RAM_END : usize = 0xc000_0000;
const PHYS_MASK : usize = 0x0000_ffff_ffff_ffff;
const ADDRESS_TAG_MASK : usize = !PHYS_MASK;
const DMW_UNCACHED : usize = 0x8000_0000_0000_0000;
const DMW_CACHED : usize = 0x9000_0000_0000_0000;

/// Normalize a firmware pointer to the cached DMW while proving the whole span
/// lies in a RAM range that early paging maps.
fn cached_dram_span(raw : usize, len : usize) -> Option<usize> {
    if raw == 0 || len == 0 {
        return None;
    }
    if raw > PHYS_MASK {
        let tag = raw & ADDRESS_TAG_MASK;
        if tag != DMW_CACHED && tag != DMW_UNCACHED {
            return None;
        }
    }
    let physical = raw & PHYS_MASK;
    let end = physical.checked_add(len)?;
    let in_low_boot_ram = physical >= LOW_BOOT_RAM_START && end <= LOW_BOOT_RAM_END;
    let in_main_ram = physical >= RAM_START && end <= RAM_END;
    (in_low_boot_ram || in_main_ram).then_some(DMW_CACHED | physical)
}

fn parse_hex_bytes(bytes : &[u8]) -> Option<usize> {
    let end = bytes.iter()
                   .position(|byte| *byte == 0)?;
    let mut digits = &bytes[..end];
    if digits.starts_with(b"0x") || digits.starts_with(b"0X") {
        digits = &digits[2..];
    }
    if digits.is_empty() {
        return None;
    }
    let mut value = 0usize;
    for byte in digits.iter().copied() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        } as usize;
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    Some(value)
}

fn parse_go_hex_arg(raw : usize) -> Option<usize> {
    let address = cached_dram_span(raw, MAX_UBOOT_GO_ARG_LEN)?;
    let mut bytes = [0u8; MAX_UBOOT_GO_ARG_LEN];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        // SAFETY: `cached_dram_span` proved the complete read lies in mapped
        // DRAM. Firmware owns the immutable argument strings during entry.
        *byte = unsafe { core::ptr::read_volatile((address + offset) as *const u8) };
    }
    parse_hex_bytes(&bytes)
}

fn push_candidate(candidates : &mut [usize], count : &mut usize, candidate : usize) {
    if *count < candidates.len() {
        candidates[*count] = candidate;
        *count += 1;
    }
}

fn append_go_arg_candidates(argc : usize,
                            argv : usize,
                            candidates : &mut [usize],
                            count : &mut usize) {
    if argc == UHI_FDT_ARG0 {
        push_candidate(candidates, count, argv);
        return;
    }
    if !(1..=MAX_UBOOT_GO_ARGS).contains(&argc) || argv == 0 {
        return;
    }
    let Some(bytes) = argc.checked_mul(core::mem::size_of::<usize>()) else {
        return;
    };
    if argv & (core::mem::align_of::<usize>() - 1) != 0 {
        return;
    }
    let Some(address) = cached_dram_span(argv, bytes) else {
        return;
    };
    let argv = address as *const usize;
    for index in 0..argc {
        // SAFETY: the complete, aligned argv array was validated above.
        let arg = unsafe { core::ptr::read_volatile(argv.add(index)) };
        if let Some(address) = parse_go_hex_arg(arg) {
            push_candidate(candidates, count, address);
        }
    }
}

fn fdt_candidate_address(candidate : usize) -> Option<usize> {
    let address = cached_dram_span(candidate, FDT_HEADER_PREFIX_LEN)?;
    let mut header = [0u8; FDT_HEADER_PREFIX_LEN];
    for (offset, byte) in header.iter_mut().enumerate() {
        // SAFETY: `cached_dram_span` proved the complete header read lies in
        // mapped DRAM. The DTB remains owned by firmware during early boot.
        *byte = unsafe { core::ptr::read_volatile((address + offset) as *const u8) };
    }
    if header[..FDT_MAGIC.len()] != FDT_MAGIC {
        return None;
    }
    let total_size = u32::from_be_bytes(header[4..8].try_into().ok()?) as usize;
    if total_size < FDT_MIN_TOTAL_SIZE {
        return None;
    }
    cached_dram_span(candidate, total_size)
}

/// Find an FDT passed by U-Boot `go` or vendor/legacy arguments.
///
/// `go entry fdt` calls the image as a C function (`a0=argc`, `a1=argv`),
/// while some vendor firmware has used a raw DTB address in one of the
/// argument registers.  Both are normalized to the cached DMW address here.
pub fn probe_dtb_from_boot_args(args : [usize; 3]) -> usize {
    let mut candidates = [0usize; MAX_UBOOT_GO_ARGS + 3];
    let mut candidate_count = 0;
    append_go_arg_candidates(args[0], args[1], &mut candidates, &mut candidate_count);
    for candidate in args {
        push_candidate(&mut candidates, &mut candidate_count, candidate);
    }

    for candidate in candidates[..candidate_count].iter().copied() {
        if let Some(address) = fdt_candidate_address(candidate) {
            return address;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_terminated_hex_arguments() {
        assert_eq!(parse_hex_bytes(b"0x9000000092000000\0"),
                   Some(0x9000_0000_9200_0000));
        assert_eq!(parse_hex_bytes(b"92000000\0"), Some(0x9200_0000));
        assert_eq!(parse_hex_bytes(b"0X2a\0"), Some(42));
        assert_eq!(parse_hex_bytes(b"0x\0"), None);
        assert_eq!(parse_hex_bytes(b"0x12z\0"), None);
        assert_eq!(parse_hex_bytes(b"92000000"), None);
    }

    #[test]
    fn validates_complete_firmware_pointer_spans() {
        assert_eq!(cached_dram_span(0x9000_0000_9200_0000, 4),
                   Some(0x9000_0000_9200_0000));
        assert_eq!(cached_dram_span(0x9200_0000, 4),
                   Some(0x9000_0000_9200_0000));
        assert_eq!(cached_dram_span(0x8000_0000_9200_0000, 4),
                   Some(0x9000_0000_9200_0000));
        assert!(cached_dram_span(0x7000_0000_9200_0000, 4).is_none());
        assert!(cached_dram_span(RAM_END - 2, 4).is_none());
        assert!(cached_dram_span(0x1fe2_0000, 4).is_none());
        assert!(cached_dram_span(usize::MAX, 4).is_none());
    }
}
