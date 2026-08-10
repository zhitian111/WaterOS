//! MBR/GPT partition discovery and bounded partition block devices.

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

use crate::{BlockDevice, DriverError, DriverResult, Lba, SharedBlockDevice, BLOCK_SIZE};

const MBR_SIGNATURE_OFFSET : usize = 510;
const MBR_PARTITION_TABLE_OFFSET : usize = 446;
const MBR_ENTRY_SIZE : usize = 16;
const MBR_ENTRY_COUNT : usize = 4;
const GPT_HEADER_LBA : u64 = 1;
const GPT_HEADER_MIN_SIZE : u32 = 92;
const GPT_ENTRY_MIN_SIZE : u32 = 128;
const GPT_ENTRY_MAX_SIZE : u32 = 4096;
const GPT_ENTRY_COUNT_MAX : u32 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MbrPartition {
    pub number : u8,
    pub partition_type : u8,
    pub start_lba : u64,
    pub sectors : u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GptPartition {
    pub number : u8,
    pub start_lba : u64,
    pub sectors : u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionScanError {
    Io(DriverError),
    InvalidBlockSize,
    InvalidSignature,
    ProtectiveGpt,
    InvalidGptHeader,
    InvalidGptEntrySize,
    InvalidGptHeaderCrc,
    InvalidGptEntryCrc,
    UnsupportedExtended,
    InvalidEntry,
    OverlappingEntries,
}

fn read_u32_le(bytes : &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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

/// Read the primary GPT header and bounded partition-entry array.
///
/// The parser validates signatures, ranges, sizes, overlap, and both primary
/// GPT header/entry-array CRCs before exposing a partition. This proves the
/// image metadata contract in software; storage-controller error injection and
/// torn-write recovery remain `UNVERIFIED_ON_HARDWARE`.
pub fn scan_gpt(device : &SharedBlockDevice) -> Result<Vec<GptPartition>, PartitionScanError> {
    let (total_blocks, mut header) = {
        let mut device = device.lock();
        if device.block_size() != BLOCK_SIZE {
            return Err(PartitionScanError::InvalidBlockSize);
        }
        let mut header = [0u8; BLOCK_SIZE];
        device.read_blocks(Lba(GPT_HEADER_LBA), &mut header)
              .map_err(PartitionScanError::Io)?;
        (device.total_blocks(), header)
    };
    if &header[0..8] != b"EFI PART" {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let header_size = read_u32_le(&header[12..16]);
    if !(GPT_HEADER_MIN_SIZE..=BLOCK_SIZE as u32).contains(&header_size)
        || read_u64_le(&header[24..32]) != GPT_HEADER_LBA
    {
        return Err(PartitionScanError::InvalidGptHeader);
    }
    let header_crc = read_u32_le(&header[16..20]);
    header[16..20].fill(0);
    if crc32(&header[..header_size as usize]) != header_crc {
        return Err(PartitionScanError::InvalidGptHeaderCrc);
    }
    let first_usable = read_u64_le(&header[40..48]);
    let last_usable = read_u64_le(&header[48..56]);
    let entries_lba = read_u64_le(&header[72..80]);
    let entry_count = read_u32_le(&header[80..84]);
    let entry_size = read_u32_le(&header[84..88]);
    if first_usable == 0 || first_usable > last_usable || entries_lba == 0
        || entry_count == 0 || entry_count > GPT_ENTRY_COUNT_MAX
        || !(GPT_ENTRY_MIN_SIZE..=GPT_ENTRY_MAX_SIZE).contains(&entry_size)
        || entry_size % 8 != 0
    {
        return Err(PartitionScanError::InvalidGptEntrySize);
    }
    let entries_bytes = u64::from(entry_count)
        .checked_mul(u64::from(entry_size))
        .ok_or(PartitionScanError::InvalidGptEntrySize)?;
    let entries_blocks = entries_bytes
        .checked_add((BLOCK_SIZE - 1) as u64)
        .ok_or(PartitionScanError::InvalidGptEntrySize)?
        / BLOCK_SIZE as u64;
    if let Some(total) = total_blocks {
        if entries_lba.checked_add(entries_blocks).filter(|end| *end <= total).is_none()
            || last_usable >= total
        {
            return Err(PartitionScanError::InvalidGptHeader);
        }
    }

    let entries_len = usize::try_from(entries_bytes)
        .map_err(|_| PartitionScanError::InvalidGptEntrySize)?;
    let entries_offset = entries_lba
        .checked_mul(BLOCK_SIZE as u64)
        .ok_or(PartitionScanError::InvalidGptHeader)?;
    let mut entries = vec![0u8; entries_len];
    device.lock()
          .read_bytes(entries_offset, &mut entries)
          .map_err(PartitionScanError::Io)?;
    if crc32(&entries) != read_u32_le(&header[88..92]) {
        return Err(PartitionScanError::InvalidGptEntryCrc);
    }

    let mut partitions = Vec::new();
    for index in 0..entry_count {
        let offset = usize::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(entry_size as usize))
            .ok_or(PartitionScanError::InvalidGptEntrySize)?;
        let entry = &entries[offset..offset + entry_size as usize];
        if entry[0..16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let start_lba = read_u64_le(&entry[32..40]);
        let end_lba = read_u64_le(&entry[40..48]);
        if start_lba < first_usable || end_lba < start_lba || end_lba > last_usable {
            return Err(PartitionScanError::InvalidEntry);
        }
        let sectors = end_lba - start_lba + 1;
        if partitions.iter().any(|prior : &GptPartition| {
            let prior_end = prior.start_lba + prior.sectors;
            start_lba < prior_end && prior.start_lba < end_lba + 1
        }) {
            return Err(PartitionScanError::OverlappingEntries);
        }
        partitions.push(GptPartition {
            number : index as u8 + 1,
            start_lba,
            sectors,
        });
    }
    Ok(partitions)
}

fn read_u64_le(bytes : &[u8]) -> u64 {
    u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7]])
}

pub(crate) fn crc32(bytes : &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
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

    fn disk_with_gpt_partition() -> SharedBlockDevice {
        let mut bytes = vec![0u8; BLOCK_SIZE * 64];
        bytes[510..512].copy_from_slice(&[0x55, 0xAA]);
        // Protective MBR entry covering the disk after LBA 0.
        bytes[MBR_PARTITION_TABLE_OFFSET + 4] = 0xEE;
        bytes[MBR_PARTITION_TABLE_OFFSET + 8..MBR_PARTITION_TABLE_OFFSET + 12]
            .copy_from_slice(&1u32.to_le_bytes());
        bytes[MBR_PARTITION_TABLE_OFFSET + 12..MBR_PARTITION_TABLE_OFFSET + 16]
            .copy_from_slice(&63u32.to_le_bytes());

        let header = &mut bytes[BLOCK_SIZE..BLOCK_SIZE * 2];
        header[0..8].copy_from_slice(b"EFI PART");
        header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        header[12..16].copy_from_slice(&92u32.to_le_bytes());
        header[24..32].copy_from_slice(&1u64.to_le_bytes());
        header[40..48].copy_from_slice(&4u64.to_le_bytes());
        header[48..56].copy_from_slice(&60u64.to_le_bytes());
        header[72..80].copy_from_slice(&2u64.to_le_bytes());
        header[80..84].copy_from_slice(&4u32.to_le_bytes());
        header[84..88].copy_from_slice(&128u32.to_le_bytes());

        let entry = &mut bytes[BLOCK_SIZE * 2..BLOCK_SIZE * 3];
        entry[0] = 1; // non-zero type GUID
        entry[32..40].copy_from_slice(&8u64.to_le_bytes());
        entry[40..48].copy_from_slice(&15u64.to_le_bytes());
        refresh_gpt_crcs(&mut bytes);
        Arc::new(Mutex::new(Box::new(MemoryDisk { bytes })))
    }

    fn refresh_gpt_crcs(bytes : &mut [u8]) {
        let entry_crc = crc32(&bytes[BLOCK_SIZE * 2..BLOCK_SIZE * 2 + 4 * 128]);
        bytes[BLOCK_SIZE + 88..BLOCK_SIZE + 92].copy_from_slice(&entry_crc.to_le_bytes());
        let mut header = [0u8; BLOCK_SIZE];
        header.copy_from_slice(&bytes[BLOCK_SIZE..BLOCK_SIZE * 2]);
        header[16..20].fill(0);
        let header_crc = crc32(&header[..92]);
        bytes[BLOCK_SIZE + 16..BLOCK_SIZE + 20].copy_from_slice(&header_crc.to_le_bytes());
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
    fn scans_gpt_partition_after_protective_mbr() {
        let disk = disk_with_gpt_partition();
        assert_eq!(scan_mbr(&disk), Err(PartitionScanError::ProtectiveGpt));
        let partitions = scan_gpt(&disk).unwrap();
        assert_eq!(partitions,
                   vec![GptPartition { number : 1,
                                       start_lba : 8,
                                       sectors : 8 }]);
    }

    #[test]
    fn rejects_gpt_partition_outside_usable_range() {
        let disk = disk_with_gpt_partition();
        let mut block = [0u8; BLOCK_SIZE];
        block[0] = 1;
        block[32..40].copy_from_slice(&3u64.to_le_bytes());
        block[40..48].copy_from_slice(&15u64.to_le_bytes());
        disk.lock()
            .write_blocks(Lba(2), &block)
            .unwrap();
        let mut entries = [0u8; BLOCK_SIZE];
        disk.lock().read_blocks(Lba(2), &mut entries).unwrap();
        let mut header = [0u8; BLOCK_SIZE];
        disk.lock().read_blocks(Lba(1), &mut header).unwrap();
        header[88..92].copy_from_slice(&crc32(&entries).to_le_bytes());
        header[16..20].fill(0);
        let header_crc = crc32(&header[..92]);
        header[16..20].copy_from_slice(&header_crc.to_le_bytes());
        disk.lock().write_blocks(Lba(1), &header).unwrap();
        assert_eq!(scan_gpt(&disk), Err(PartitionScanError::InvalidEntry));
    }

    #[test]
    fn rejects_bad_gpt_checksums() {
        let header_corrupt = disk_with_gpt_partition();
        let mut header = [0u8; BLOCK_SIZE];
        header_corrupt.lock().read_blocks(Lba(1), &mut header).unwrap();
        header[60] ^= 1;
        header_corrupt.lock().write_blocks(Lba(1), &header).unwrap();
        assert_eq!(scan_gpt(&header_corrupt), Err(PartitionScanError::InvalidGptHeaderCrc));

        let entry_corrupt = disk_with_gpt_partition();
        let mut block = [0u8; BLOCK_SIZE];
        entry_corrupt.lock().read_blocks(Lba(2), &mut block).unwrap();
        block[0] = 2;
        entry_corrupt.lock().write_blocks(Lba(2), &block).unwrap();
        assert_eq!(scan_gpt(&entry_corrupt), Err(PartitionScanError::InvalidGptEntryCrc));
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
}
