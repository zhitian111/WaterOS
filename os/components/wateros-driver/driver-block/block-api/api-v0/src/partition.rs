//! MBR partition discovery and bounded partition block devices.

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{BlockDevice, DriverError, DriverResult, Lba, SharedBlockDevice, BLOCK_SIZE};

const MBR_SIGNATURE_OFFSET : usize = 510;
const MBR_PARTITION_TABLE_OFFSET : usize = 446;
const MBR_ENTRY_SIZE : usize = 16;
const MBR_ENTRY_COUNT : usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MbrPartition {
    pub number : u8,
    pub partition_type : u8,
    pub start_lba : u64,
    pub sectors : u64,
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
}
