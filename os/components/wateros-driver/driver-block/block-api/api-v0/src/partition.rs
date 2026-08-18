//! MBR partition discovery and bounded partition block devices.

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

use crate::{BlockDevice, DriverError, DriverResult, Lba, SharedBlockDevice, BLOCK_SIZE};

const MBR_SIGNATURE_OFFSET : usize = 510;
const MBR_PARTITION_TABLE_OFFSET : usize = 446;
const MBR_ENTRY_SIZE : usize = 16;
const MBR_ENTRY_COUNT : usize = 4;
const GPT_SIGNATURE : &[u8; 8] = b"EFI PART";
const GPT_MIN_HEADER_SIZE : usize = 92;
const GPT_MAX_ENTRY_COUNT : u32 = 4096;
const GPT_MIN_ENTRY_SIZE : u32 = 128;
const GPT_MAX_ENTRY_SIZE : u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MbrPartition {
    pub number : u8,
    pub partition_type : u8,
    pub start_lba : u64,
    pub sectors : u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GptPartition {
    pub number : u32,
    pub start_lba : u64,
    pub end_lba : u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionScanError {
    Io(DriverError),
    InvalidBlockSize,
    InvalidSignature,
    ProtectiveGpt,
    UnsupportedExtended,
    InvalidEntry,
    OverlappingEntries,
    InvalidGptHeader,
    GptHeaderCrc,
    GptEntryCrc,
    UnsupportedGptLayout,
    GptEntryOutOfRange,
}

fn read_u32_le(bytes : &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_u64_le(bytes : &[u8]) -> u64 {
    u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]])
}

fn crc32(bytes : &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Read the legacy MBR primary partition table.
///
/// GPT protective and extended partitions are reported explicitly instead of
/// being exposed as ordinary data partitions.
pub fn scan_mbr(device : &SharedBlockDevice) -> Result<Vec<MbrPartition>, PartitionScanError> {
    let mut sector = [0u8; BLOCK_SIZE];
    let total_blocks = {
        let mut device = device.lock();
        if device.block_size() != BLOCK_SIZE {
            return Err(PartitionScanError::InvalidBlockSize);
        }
        device.read_blocks(Lba(0), &mut sector)
              .map_err(PartitionScanError::Io)?;
        device.total_blocks()
    };
    if sector[MBR_SIGNATURE_OFFSET..] != [0x55, 0xAA] {
        return Err(PartitionScanError::InvalidSignature);
    }

    let mut partitions = Vec::new();
    for index in 0..MBR_ENTRY_COUNT {
        let offset = MBR_PARTITION_TABLE_OFFSET + index * MBR_ENTRY_SIZE;
        let entry = &sector[offset..offset + MBR_ENTRY_SIZE];
        let partition_type = entry[4];
        let start_lba = read_u32_le(&entry[8..12]) as u64;
        let sectors = read_u32_le(&entry[12..16]) as u64;
        if partition_type == 0 && start_lba == 0 && sectors == 0 {
            continue;
        }
        if partition_type == 0xEE {
            return Err(PartitionScanError::ProtectiveGpt);
        }
        if matches!(partition_type, 0x05 | 0x0F | 0x85) {
            return Err(PartitionScanError::UnsupportedExtended);
        }
        let end = start_lba.checked_add(sectors)
                           .ok_or(PartitionScanError::InvalidEntry)?;
        if partition_type == 0 || start_lba == 0 || sectors == 0 {
            return Err(PartitionScanError::InvalidEntry);
        }
        if let Some(total) = total_blocks {
            if end > total {
                return Err(PartitionScanError::InvalidEntry);
            }
        }
        if partitions.iter()
                     .any(|prior : &MbrPartition| {
                         let prior_end = prior.start_lba + prior.sectors;
                         start_lba < prior_end && prior.start_lba < end
                     })
        {
            return Err(PartitionScanError::OverlappingEntries);
        }
        partitions.push(MbrPartition { number : index as u8 + 1,
                                       partition_type,
                                       start_lba,
                                       sectors });
    }
    Ok(partitions)
}

/// Read a GPT header and its partition-entry array without modifying the disk.
/// Empty entries are skipped; populated entries are range-checked and rejected
/// if they overlap. The result is intentionally separate from MBR registration
/// until the public device-number contract grows a GPT-specific role.
fn scan_gpt_at(device : &SharedBlockDevice,
               header_lba : u64,
               total_blocks : u64)
               -> Result<Vec<GptPartition>, PartitionScanError> {
    let mut header = [0u8; BLOCK_SIZE];
    device.lock().read_blocks(Lba(header_lba), &mut header).map_err(PartitionScanError::Io)?;
    if &header[..8] != GPT_SIGNATURE {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let header_size = read_u32_le(&header[12..16]) as usize;
    if !(GPT_MIN_HEADER_SIZE..=BLOCK_SIZE).contains(&header_size) ||
       read_u64_le(&header[24..32]) != header_lba ||
       read_u64_le(&header[72..80]) == 0 ||
       read_u32_le(&header[80..84]) == 0
    {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let backup_lba = read_u64_le(&header[32..40]);
    let first_usable = read_u64_le(&header[40..48]);
    let last_usable = read_u64_le(&header[48..56]);
    if backup_lba >= total_blocks ||
       first_usable > last_usable || last_usable >= total_blocks {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let expected_header_crc = read_u32_le(&header[16..20]);
    let mut crc_header = header[..header_size].to_vec();
    crc_header[16..20].fill(0);
    if crc32(&crc_header) != expected_header_crc {
        return Err(PartitionScanError::GptHeaderCrc);
    }

    let entry_lba = read_u64_le(&header[72..80]);
    let entry_count = read_u32_le(&header[80..84]);
    let entry_size = read_u32_le(&header[84..88]);
    if !(GPT_MIN_ENTRY_SIZE..=GPT_MAX_ENTRY_SIZE).contains(&entry_size) ||
       !entry_size.is_multiple_of(8) || entry_count > GPT_MAX_ENTRY_COUNT
    {
        return Err(PartitionScanError::UnsupportedGptLayout);
    }
    let entry_bytes = usize::try_from(entry_count)
        .ok().and_then(|count| usize::try_from(entry_size).ok().and_then(|size| count.checked_mul(size)))
        .ok_or(PartitionScanError::UnsupportedGptLayout)?;
    let entry_sectors = entry_bytes.div_ceil(BLOCK_SIZE) as u64;
    if entry_lba == 0 || entry_lba.checked_add(entry_sectors).is_none_or(|end| end > total_blocks) {
        return Err(PartitionScanError::GptEntryOutOfRange);
    }
    let mut entries = vec![0u8; entry_bytes];
    {
        let mut device = device.lock();
        let entry_offset = entry_lba.checked_mul(BLOCK_SIZE as u64)
                                      .ok_or(PartitionScanError::GptEntryOutOfRange)?;
        device.read_bytes(entry_offset, &mut entries)
              .map_err(PartitionScanError::Io)?;
    }
    if crc32(&entries) != read_u32_le(&header[88..92]) {
        return Err(PartitionScanError::GptEntryCrc);
    }
    let mut partitions = Vec::new();
    for index in 0..entry_count as usize {
        let offset = index * entry_size as usize;
        let entry = &entries[offset..offset + entry_size as usize];
        if entry[..16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let start_lba = read_u64_le(&entry[32..40]);
        let end_lba = read_u64_le(&entry[40..48]);
        if start_lba < first_usable || start_lba > end_lba || end_lba > last_usable {
            return Err(PartitionScanError::GptEntryOutOfRange);
        }
        if partitions.iter().any(|prior : &GptPartition| {
            start_lba <= prior.end_lba && prior.start_lba <= end_lba
        }) {
            return Err(PartitionScanError::OverlappingEntries);
        }
        partitions.push(GptPartition { number : index as u32 + 1, start_lba, end_lba });
    }
    Ok(partitions)
}

/// Read a GPT primary header, falling back once to the backup header at the
/// end of the disk when the primary is damaged or incomplete.
pub fn scan_gpt(device : &SharedBlockDevice) -> Result<Vec<GptPartition>, PartitionScanError> {
    let total_blocks = {
        let device = device.lock();
        if device.block_size() != BLOCK_SIZE {
            return Err(PartitionScanError::InvalidBlockSize);
        }
        device.total_blocks().ok_or(PartitionScanError::InvalidGptHeader)?
    };
    if total_blocks < 2 {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let primary = scan_gpt_at(device, 1, total_blocks);
    match primary {
        Ok(partitions) => Ok(partitions),
        Err(primary_error) => scan_gpt_at(device, total_blocks - 1, total_blocks)
            .map_err(|_| primary_error),
    }
}

pub struct PartitionBlockDevice {
    parent : SharedBlockDevice,
    start_lba : u64,
    sectors : u64,
    block_size : usize,
}

impl PartitionBlockDevice {
    pub fn new(parent : SharedBlockDevice, start_lba : u64, sectors : u64) -> DriverResult<Self> {
        if start_lba == 0 || sectors == 0 {
            return Err(DriverError::InvalidParam);
        }
        let parent_guard = parent.lock();
        let block_size = parent_guard.block_size();
        if block_size == 0 {
            return Err(DriverError::InvalidParam);
        }
        if let Some(total) = parent_guard.total_blocks() {
            if start_lba.checked_add(sectors)
                        .filter(|end| *end <= total)
                        .is_none()
            {
                return Err(DriverError::InvalidParam);
            }
        }
        drop(parent_guard);
        Ok(Self { parent,
                  start_lba,
                  sectors,
                  block_size })
    }

    pub fn shared(parent : SharedBlockDevice,
                  start_lba : u64,
                  sectors : u64)
                  -> DriverResult<SharedBlockDevice> {
        let partition : Box<dyn BlockDevice> = Box::new(Self::new(parent, start_lba, sectors)?);
        Ok(Arc::new(Mutex::new(partition)))
    }

    fn translated_start(&self, start : Lba, byte_len : usize) -> DriverResult<Lba> {
        if byte_len % self.block_size != 0 {
            return Err(DriverError::InvalidParam);
        }
        let count =
            u64::try_from(byte_len / self.block_size).map_err(|_| DriverError::InvalidParam)?;
        let end = start.0
                       .checked_add(count)
                       .ok_or(DriverError::InvalidParam)?;
        if end > self.sectors {
            return Err(DriverError::InvalidParam);
        }
        self.start_lba
            .checked_add(start.0)
            .map(Lba)
            .ok_or(DriverError::InvalidParam)
    }
}

impl BlockDevice for PartitionBlockDevice {
    fn block_size(&self) -> usize { self.block_size }
    fn total_blocks(&self) -> Option<u64> { Some(self.sectors) }

    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        let parent_start = self.translated_start(start_block, buf.len())?;
        self.parent
            .lock()
            .read_blocks(parent_start, buf)
    }

    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let parent_start = self.translated_start(start_block, buf.len())?;
        self.parent
            .lock()
            .write_blocks(parent_start, buf)
    }

    fn flush(&mut self) -> DriverResult<()> {
        self.parent.lock().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    struct MemoryDisk {
        bytes : Vec<u8>,
    }

    impl BlockDevice for MemoryDisk {
        fn total_blocks(&self) -> Option<u64> { Some((self.bytes.len() / BLOCK_SIZE) as u64) }
        fn read_blocks(&mut self, start : Lba, buf : &mut [u8]) -> DriverResult<()> {
            let start = start.0 as usize * BLOCK_SIZE;
            let source = self.bytes
                             .get(start..start + buf.len())
                             .ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(source);
            Ok(())
        }
        fn write_blocks(&mut self, start : Lba, buf : &[u8]) -> DriverResult<()> {
            let start = start.0 as usize * BLOCK_SIZE;
            let destination = self.bytes
                                  .get_mut(start..start + buf.len())
                                  .ok_or(DriverError::InvalidParam)?;
            destination.copy_from_slice(buf);
            Ok(())
        }

        fn flush(&mut self) -> DriverResult<()> { Ok(()) }
    }

    fn disk_with_entries(entries : &[(u8, u32, u32)]) -> SharedBlockDevice {
        let mut bytes = vec![0u8; BLOCK_SIZE * 32];
        bytes[510..512].copy_from_slice(&[0x55, 0xAA]);
        for (index, (kind, start, sectors)) in entries.iter()
                                                      .enumerate()
        {
            let offset = MBR_PARTITION_TABLE_OFFSET + index * MBR_ENTRY_SIZE;
            bytes[offset + 4] = *kind;
            bytes[offset + 8..offset + 12].copy_from_slice(&start.to_le_bytes());
            bytes[offset + 12..offset + 16].copy_from_slice(&sectors.to_le_bytes());
        }
        Arc::new(Mutex::new(Box::new(MemoryDisk { bytes })))
    }

    fn gpt_disk(ranges : &[(u64, u64)]) -> SharedBlockDevice {
        let mut bytes = vec![0u8; BLOCK_SIZE * 32];
        let entries = &mut bytes[BLOCK_SIZE * 2..BLOCK_SIZE * 3];
        for (index, (start, end)) in ranges.iter().enumerate() {
            let entry = &mut entries[index * 128..(index + 1) * 128];
            entry[0] = 1;
            entry[32..40].copy_from_slice(&start.to_le_bytes());
            entry[40..48].copy_from_slice(&end.to_le_bytes());
        }
        let entries_crc = crc32(entries);
        let header = &mut bytes[BLOCK_SIZE..BLOCK_SIZE * 2];
        header[..8].copy_from_slice(GPT_SIGNATURE);
        header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        header[12..16].copy_from_slice(&(GPT_MIN_HEADER_SIZE as u32).to_le_bytes());
        header[24..32].copy_from_slice(&1u64.to_le_bytes());
        header[32..40].copy_from_slice(&31u64.to_le_bytes());
        header[40..48].copy_from_slice(&4u64.to_le_bytes());
        header[48..56].copy_from_slice(&28u64.to_le_bytes());
        header[72..80].copy_from_slice(&2u64.to_le_bytes());
        header[80..84].copy_from_slice(&4u32.to_le_bytes());
        header[84..88].copy_from_slice(&128u32.to_le_bytes());
        header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let header_crc = crc32(&header[..GPT_MIN_HEADER_SIZE]);
        header[16..20].copy_from_slice(&header_crc.to_le_bytes());
        let primary_entries = bytes[BLOCK_SIZE * 2..BLOCK_SIZE * 3].to_vec();
        bytes[BLOCK_SIZE * 30..BLOCK_SIZE * 31].copy_from_slice(&primary_entries);
        let backup = &mut bytes[BLOCK_SIZE * 31..BLOCK_SIZE * 32];
        backup[..8].copy_from_slice(GPT_SIGNATURE);
        backup[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        backup[12..16].copy_from_slice(&(GPT_MIN_HEADER_SIZE as u32).to_le_bytes());
        backup[24..32].copy_from_slice(&31u64.to_le_bytes());
        backup[32..40].copy_from_slice(&1u64.to_le_bytes());
        backup[40..48].copy_from_slice(&4u64.to_le_bytes());
        backup[48..56].copy_from_slice(&28u64.to_le_bytes());
        backup[72..80].copy_from_slice(&30u64.to_le_bytes());
        backup[80..84].copy_from_slice(&4u32.to_le_bytes());
        backup[84..88].copy_from_slice(&128u32.to_le_bytes());
        backup[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let backup_crc = crc32(&backup[..GPT_MIN_HEADER_SIZE]);
        backup[16..20].copy_from_slice(&backup_crc.to_le_bytes());
        Arc::new(Mutex::new(Box::new(MemoryDisk { bytes })))
    }

    #[test]
    fn scans_primary_partitions() {
        let disk = disk_with_entries(&[(0x83, 1, 4),
                                       (0x0C, 8, 8)]);
        let partitions = scan_mbr(&disk).unwrap();
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[1].number, 2);
        assert_eq!(partitions[1].start_lba, 8);
    }

    #[test]
    fn partition_translates_and_checks_bounds() {
        let disk = disk_with_entries(&[(0x83, 2, 3)]);
        let mut partition = PartitionBlockDevice::new(disk.clone(), 2, 3).unwrap();
        partition.write_blocks(Lba(1), &[0xA5; BLOCK_SIZE])
                 .unwrap();
        let mut parent = [0u8; BLOCK_SIZE];
        disk.lock()
            .read_blocks(Lba(3), &mut parent)
            .unwrap();
        assert!(parent.iter()
                      .all(|byte| *byte == 0xA5));
        assert_eq!(partition.read_blocks(Lba(3), &mut parent),
                   Err(DriverError::InvalidParam));
    }

    #[test]
    fn rejects_bad_or_unsupported_tables() {
        let signature = disk_with_entries(&[(0x83, 1, 4)]);
        signature.lock()
                 .write_blocks(Lba(0), &[0u8; BLOCK_SIZE])
                 .unwrap();
        assert_eq!(scan_mbr(&signature),
                   Err(PartitionScanError::InvalidSignature));
        let bad = disk_with_entries(&[(0x83, 30, 4)]);
        assert_eq!(scan_mbr(&bad),
                   Err(PartitionScanError::InvalidEntry));
        let gpt = disk_with_entries(&[(0xEE, 1, 31)]);
        assert_eq!(scan_mbr(&gpt),
                   Err(PartitionScanError::ProtectiveGpt));
        let extended = disk_with_entries(&[(0x0F, 1, 16)]);
        assert_eq!(scan_mbr(&extended),
                   Err(PartitionScanError::UnsupportedExtended));
        let overlap = disk_with_entries(&[(0x83, 1, 8),
                                          (0x83, 4, 8)]);
        assert_eq!(scan_mbr(&overlap),
                   Err(PartitionScanError::OverlappingEntries));
    }

    #[test]
    fn scans_gpt_entries_and_rejects_corruption() {
        let disk = gpt_disk(&[(4, 7), (12, 20)]);
        let partitions = scan_gpt(&disk).unwrap();
        assert_eq!(partitions,
                   vec![GptPartition { number : 1, start_lba : 4, end_lba : 7 },
                        GptPartition { number : 2, start_lba : 12, end_lba : 20 }]);

        let primary_bad = gpt_disk(&[(4, 7)]);
        primary_bad.lock().write_blocks(Lba(1), &[0; BLOCK_SIZE]).unwrap();
        assert_eq!(scan_gpt(&primary_bad).unwrap().len(), 1);
        let both_bad = gpt_disk(&[(4, 7)]);
        both_bad.lock().write_blocks(Lba(1), &[0; BLOCK_SIZE]).unwrap();
        both_bad.lock().write_blocks(Lba(31), &[0; BLOCK_SIZE]).unwrap();
        assert_eq!(scan_gpt(&both_bad), Err(PartitionScanError::InvalidGptHeader));

        let overlap = gpt_disk(&[(4, 10), (8, 12)]);
        assert_eq!(scan_gpt(&overlap), Err(PartitionScanError::OverlappingEntries));
        let out_of_range = gpt_disk(&[(3, 5)]);
        assert_eq!(scan_gpt(&out_of_range), Err(PartitionScanError::GptEntryOutOfRange));
    }
}
