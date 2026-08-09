//! Loongson 2K1000LA unified-system-architecture boot parameters.

use core::{mem, slice};

use api_v0::boot::PlatformBootArgs;

const UEFI_COMPATIBLE_BOOT : usize = 1;
const EFI_SYSTEM_TABLE_SIGNATURE : u64 = 0x5453_5953_2049_4249;
const MAX_CONFIGURATION_TABLE_ENTRIES : usize = 4096;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EfiGuid {
    data1 : u32,
    data2 : u16,
    data3 : u16,
    data4 : [u8; 8],
}

const DEVICE_TREE_GUID : EfiGuid = EfiGuid { data1 : 0xB1B6_21D5,
                                             data2 : 0xF19C,
                                             data3 : 0x41A5,
                                             data4 : [0x83, 0x0B, 0xD9, 0x15, 0x2C, 0x69, 0xAA,
                                                      0xE0] };

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct EfiTableHeader {
    signature : u64,
    revision : u32,
    header_size : u32,
    crc32 : u32,
    reserved : u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct EfiConfigurationTable {
    vendor_guid : EfiGuid,
    vendor_table : usize,
}

#[repr(C)]
struct EfiSystemTable {
    header : EfiTableHeader,
    firmware_vendor : usize,
    firmware_revision : u32,
    _padding : u32,
    console_in_handle : usize,
    console_in : usize,
    console_out_handle : usize,
    console_out : usize,
    standard_error_handle : usize,
    standard_error : usize,
    runtime_services : usize,
    boot_services : usize,
    configuration_table_entry_count : usize,
    configuration_table : *const EfiConfigurationTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DtbDiscoveryError {
    UnsupportedBootProtocol,
    MissingSystemTable,
    MisalignedSystemTable,
    InvalidSystemTable,
    InvalidConfigurationTable,
    MissingDeviceTree,
}

fn find_device_tree(entries : &[EfiConfigurationTable]) -> Option<usize> {
    entries.iter()
           .find(|entry| entry.vendor_guid == DEVICE_TREE_GUID && entry.vendor_table != 0)
           .map(|entry| entry.vendor_table)
}

/// Finds the FDT installed by firmware in the UEFI Configuration Table.
///
/// # Safety
///
/// The caller must ensure `system_table_pa` and the configuration-table range
/// named by it remain identity-mapped, readable firmware memory during this
/// call. Header, alignment, null and length checks bound the data interpreted
/// after that platform guarantee has been established.
unsafe fn discover_device_tree(arg0 : usize,
                               system_table_pa : usize)
                               -> Result<usize, DtbDiscoveryError> {
    if arg0 != UEFI_COMPATIBLE_BOOT {
        return Err(DtbDiscoveryError::UnsupportedBootProtocol);
    }
    if system_table_pa == 0 {
        return Err(DtbDiscoveryError::MissingSystemTable);
    }
    if !system_table_pa.is_multiple_of(64 * 1024) {
        return Err(DtbDiscoveryError::MisalignedSystemTable);
    }

    // SAFETY: The function contract requires readable firmware memory at this address.
    let system_table = unsafe { &*(system_table_pa as *const EfiSystemTable) };
    if system_table.header
                   .signature !=
       EFI_SYSTEM_TABLE_SIGNATURE ||
       (system_table.header
                    .header_size as usize) <
       mem::size_of::<EfiSystemTable>()
    {
        return Err(DtbDiscoveryError::InvalidSystemTable);
    }

    let count = system_table.configuration_table_entry_count;
    if count > MAX_CONFIGURATION_TABLE_ENTRIES ||
       (count != 0 &&
        system_table.configuration_table
                    .is_null())
    {
        return Err(DtbDiscoveryError::InvalidConfigurationTable);
    }
    let entries = if count == 0 {
        &[]
    } else {
        // SAFETY: The function contract covers this firmware-owned range; it is non-null and the
        // count is bounded.
        unsafe { slice::from_raw_parts(system_table.configuration_table, count) }
    };
    find_device_tree(entries).ok_or(DtbDiscoveryError::MissingDeviceTree)
}

/// Returns the FDT physical address described by the 2K1000LA firmware ABI.
///
/// `arg1` is the command-line physical address and is intentionally not parsed
/// here. Returning zero preserves the platform layer's established "no DTB"
/// sentinel when firmware input fails validation.
pub fn device_tree_phys_addr(arg0 : usize, _arg1 : usize, arg2 : usize) -> usize {
    // SAFETY: At the architecture entry this is still the firmware-provided,
    // identity-addressable EFI System Table. Validation occurs before following
    // its configuration table pointer.
    unsafe { discover_device_tree(arg0, arg2).unwrap_or(0) }
}

#[derive(Debug, Clone, Copy)]
pub struct Loongson2K1000LABootArgs {
    uefi_compatible : usize,
    command_line_pa : usize,
    system_table_pa : usize,
}

impl Loongson2K1000LABootArgs {
    pub const fn new(uefi_compatible : usize,
                     command_line_pa : usize,
                     system_table_pa : usize)
                     -> Self {
        Self { uefi_compatible,
               command_line_pa,
               system_table_pa }
    }
}

impl PlatformBootArgs for Loongson2K1000LABootArgs {
    fn arg0(&self) -> Option<usize> { Some(self.uefi_compatible) }
    fn arg1(&self) -> Option<usize> { Some(self.command_line_pa) }
    fn arg2(&self) -> Option<usize> { Some(self.system_table_pa) }
}

pub use Loongson2K1000LABootArgs as BootArgs;

#[cfg(test)]
mod tests {
    use super::*;

    const OTHER_GUID : EfiGuid = EfiGuid { data1 : 1,
                                           data2 : 2,
                                           data3 : 3,
                                           data4 : [4; 8] };

    #[test]
    fn finds_non_first_device_tree_entry() {
        let entries = [EfiConfigurationTable { vendor_guid : OTHER_GUID,
                                               vendor_table : 0x1000 },
                       EfiConfigurationTable { vendor_guid : DEVICE_TREE_GUID,
                                               vendor_table : 0x20_0000 }];
        assert_eq!(find_device_tree(&entries),
                   Some(0x20_0000));
    }

    #[test]
    fn rejects_null_or_missing_device_tree_entry() {
        let entries = [EfiConfigurationTable { vendor_guid : DEVICE_TREE_GUID,
                                               vendor_table : 0 },
                       EfiConfigurationTable { vendor_guid : OTHER_GUID,
                                               vendor_table : 0x1000 }];
        assert_eq!(find_device_tree(&entries), None);
    }
}
