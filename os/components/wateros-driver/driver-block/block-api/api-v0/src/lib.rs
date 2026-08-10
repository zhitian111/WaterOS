//! 块设备抽象：逻辑块寻址、全局注册表与默认块大小常量。
//!
//! [`BlockDevice`] 提供按块与按字节读取的默认实现；写路径由具体设备决定是否支持。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

pub mod partition;
pub use partition::{GptPartition, MbrPartition, PartitionBlockDevice, PartitionScanError, scan_gpt,
                    scan_mbr};

pub use driver_api::{DriverError, DriverResult, device_topology_generation};

/// 逻辑块字节长度；当前 WaterOS bring-up 固定为 512（与 virtio-blk 常见配置一致）。
pub const BLOCK_SIZE: usize = 512;

/// 逻辑块地址（LBA），从 0 起算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lba(pub u64);

impl From<usize> for Lba {
    /// 将 `usize` 截断/拓宽为 `u64` LBA（与平台指针宽度一致的内核路径常用）。
    fn from(value: usize) -> Self { Self(value as u64) }
}

impl From<u64> for Lba {
    /// 直接包装为 LBA，无额外校验（非法 LBA 由具体设备在读时拒绝）。
    fn from(value: u64) -> Self { Self(value) }
}

/// 可在多任务间共享的块设备句柄（内部可变性由 `spin::Mutex` 提供）。
pub type SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>;

// 注册顺序稳定：`register_block_device` 返回的下标即在此 `Vec` 中的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockDeviceRole {
    Disk { disk_number : usize },
    Partition { parent_device_index : usize, partition_number : u32 },
}

struct RegisteredBlockDevice {
    device : SharedBlockDevice,
    role : BlockDeviceRole,
}

static BLOCK_DEVICES : Mutex<Vec<Option<RegisteredBlockDevice>>> = Mutex::new(Vec::new());

/// 块设备语义契约：按块读写为必须实现；按字节读提供默认实现（内部临时缓冲整段块）。
pub trait BlockDevice: Send {
    /// 设备逻辑块大小；默认 [`BLOCK_SIZE`]。
    fn block_size(&self) -> usize { BLOCK_SIZE }

    /// 设备总块数；未知时返回 `None`（默认）。
    fn total_blocks(&self) -> Option<u64> { None }

    /// 从 `start_block` 起读取连续块到 `buf`；`buf` 长度须为块大小的整数倍。
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()>;

    /// 从 `start_block` 起写入连续块；不支持写时须返回 [`DriverError::Unsupported`]。
    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()>;

    /// 任意字节对齐读取：通过整段块缓冲实现，调用方须保证 `dst` 非空且偏移合法。
    fn read_bytes(&mut self, offset: u64, dst: &mut [u8]) -> DriverResult<()> {
        if dst.is_empty() {
            return Ok(());
        }

        let block_size = self.block_size();
        if block_size == 0 {
            return Err(DriverError::InvalidParam);
        }

        // `offset` 须可落入 `usize`：内核 bring-up 路径通常远低于平台地址空间上限。
        let start_byte = usize::try_from(offset).map_err(|_| DriverError::InvalidParam)?;
        let end_byte = start_byte
            .checked_add(dst.len())
            .ok_or(DriverError::InvalidParam)?;
        let start_block = start_byte / block_size;
        let end_block = end_byte.div_ceil(block_size);
        let block_count = end_block
            .checked_sub(start_block)
            .ok_or(DriverError::InvalidParam)?;
        let scratch_len = block_count
            .checked_mul(block_size)
            .ok_or(DriverError::InvalidParam)?;
        // 临时覆盖跨块区间；设备侧仍只看到整倍数 `block_size` 的 `read_blocks` 调用。
        let mut scratch = vec![0u8; scratch_len];

        self.read_blocks(Lba(start_block as u64), &mut scratch)?;

        let offset_in_block = start_byte % block_size;
        let read_end = offset_in_block
            .checked_add(dst.len())
            .ok_or(DriverError::InvalidParam)?;
        dst.copy_from_slice(&scratch[offset_in_block..read_end]);
        Ok(())
    }

    /// 读取从 `offset` 起的 `len` 字节到新分配的缓冲区。
    fn read_prefix(&mut self, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read_bytes(offset, &mut buf)?;
        Ok(buf)
    }
}

/// 将整盘追加到全局表末尾，返回整盘索引（从 0 起）。
///
/// 若首扇区包含受支持的 MBR 主分区表，对应的有界分区设备会紧随整盘注册。
pub fn register_block_device(device: SharedBlockDevice) -> usize {
    let scan = scan_mbr(&device);
    let mut children = Vec::new();
    match &scan {
        Ok(partitions) => {
            for partition in partitions {
                if let Ok(child) = PartitionBlockDevice::shared(device.clone(),
                                                                partition.start_lba,
                                                                partition.sectors)
                {
                    children.push((partition.number as u32, child));
                }
            }
        }
        Err(PartitionScanError::ProtectiveGpt) => {
            if let Ok(partitions) = scan_gpt(&device) {
                for partition in partitions {
                    if let Some(sectors) = partition.end_lba
                                                     .checked_sub(partition.start_lba)
                                                     .and_then(|count| count.checked_add(1))
                {
                    if let Ok(child) = PartitionBlockDevice::shared(device.clone(),
                                                                    partition.start_lba,
                                                                    sectors)
                    {
                        children.push((partition.number, child));
                    }
                }
                }
            }
        }
        Err(_) => {}
    }

    let mut devices = BLOCK_DEVICES.lock();
    let disk_number = first_available_disk_number(&devices);
    let disk_index = devices.len();
    devices.push(Some(RegisteredBlockDevice { device : device.clone(),
                                              role : BlockDeviceRole::Disk { disk_number } }));
    for (partition_number, child) in children {
        devices.push(Some(RegisteredBlockDevice {
            device : child,
            role : BlockDeviceRole::Partition { parent_device_index : disk_index,
                                                partition_number },
        }));
    }
    drop(devices);
    driver_api::notify_device_topology_changed();

    match scan {
        Ok(_) => {},
        // 没有 MBR 签名是合法的整盘文件系统布局，不需要告警。
        Err(PartitionScanError::InvalidSignature) => {},
        Err(error) => {
            #[cfg(feature = "logging")]
            logging::warn!("[driver-block-api] disk #{disk_number} partition scan skipped: {error:?}");
            #[cfg(not(feature = "logging"))]
            let _ = error;
        },
    }
    disk_index
}

fn first_available_disk_number(devices : &[Option<RegisteredBlockDevice>]) -> usize {
    (0..).find(|candidate| {
             !devices.iter().flatten().any(|entry| {
                 matches!(entry.role,
                          BlockDeviceRole::Disk { disk_number } if disk_number == *candidate)
             })
         })
         .expect("finite registry always has a free disk number")
}

/// 当前已注册块设备数量，包括整盘与自动发现的分区设备。
pub fn block_device_count() -> usize {
    BLOCK_DEVICES.lock().iter().flatten().count()
}

/// 取表中第一个设备，常用于根文件系统绑定单盘场景。
pub fn first_block_device() -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock().iter().flatten().next().map(|entry| entry.device.clone())
}

/// 按下标取设备；越界返回 `None`。
pub fn block_device_at(index: usize) -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock().get(index).and_then(Option::as_ref).map(|entry| entry.device.clone())
}

pub fn block_device_role_at(index : usize) -> Option<BlockDeviceRole> {
    BLOCK_DEVICES.lock().get(index).and_then(Option::as_ref).map(|entry| entry.role)
}

/// Snapshot active devices without retaining the registry lock.
pub fn block_devices_snapshot() -> Vec<(usize, SharedBlockDevice, BlockDeviceRole)> {
    BLOCK_DEVICES.lock()
                 .iter()
                 .enumerate()
                 .filter_map(|(index, entry)| {
                     entry.as_ref().map(|entry| (index, entry.device.clone(), entry.role))
                 })
                 .collect()
}

/// Remove a registry slot. Removing a disk also removes all child partitions.
///
/// Existing [`SharedBlockDevice`] clones remain alive. A physical hot-unplug
/// driver must quiesce DMA and make outstanding I/O fail before calling this;
/// that hardware sequence is not testable without the target board.
pub fn unregister_block_device(index : usize) -> bool {
    let mut devices = BLOCK_DEVICES.lock();
    let Some(role) = devices.get(index).and_then(Option::as_ref).map(|entry| entry.role) else {
        return false;
    };
    devices[index] = None;
    if matches!(role, BlockDeviceRole::Disk { .. }) {
        for entry in devices.iter_mut() {
            if entry.as_ref().is_some_and(|entry| {
                matches!(entry.role,
                         BlockDeviceRole::Partition { parent_device_index, .. }
                         if parent_device_index == index)
            }) {
                *entry = None;
            }
        }
    }
    drop(devices);
    driver_api::notify_device_topology_changed();
    true
}

/// 自检：校验常量与样例设备的 [`read_prefix`] 行为。
pub fn test() {
    #[cfg(feature = "logging")]
    logging::trace!("[driver-block-api] test begin");
    assert_eq!(BLOCK_SIZE, 512);
    let mut sample = SampleBlockDevice::new();
    let prefix = sample.read_prefix(3, 5).expect("prefix read should work");
    assert_eq!(&prefix, &[3, 4, 5, 6, 7]);
    #[cfg(feature = "logging")]
    logging::trace!("[driver-block-api] test end");
}

// 内存中的连续字节数组模拟两块设备；`read_blocks` 按字节偏移切片，写路径恒不支持。
struct SampleBlockDevice {
    bytes: [u8; BLOCK_SIZE * 2],
}

impl SampleBlockDevice {
    // 字节值等于下标 mod 256，便于 `read_prefix` 断言可读内容可预测。
    fn new() -> Self {
        let mut bytes = [0u8; BLOCK_SIZE * 2];
        for (idx, value) in bytes.iter_mut().enumerate() {
            *value = idx as u8;
        }
        Self { bytes }
    }
}

impl BlockDevice for SampleBlockDevice {
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        if buf.len() % BLOCK_SIZE != 0 {
            return Err(DriverError::InvalidParam);
        }
        let start = usize::try_from(start_block.0)
            .map_err(|_| DriverError::InvalidParam)?
            .checked_mul(BLOCK_SIZE)
            .ok_or(DriverError::InvalidParam)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(DriverError::InvalidParam)?;
        let src = self.bytes.get(start..end).ok_or(DriverError::InvalidParam)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write_blocks(&mut self, _start_block: Lba, _buf: &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    struct RegistryDisk {
        bytes : Vec<u8>,
    }

    impl RegistryDisk {
        fn shared_with_partition() -> SharedBlockDevice {
            let mut bytes = vec![0u8; BLOCK_SIZE * 8];
            bytes[510..512].copy_from_slice(&[0x55, 0xAA]);
            bytes[446 + 4] = 0x83;
            bytes[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
            bytes[446 + 12..446 + 16].copy_from_slice(&2u32.to_le_bytes());
            Arc::new(Mutex::new(Box::new(Self { bytes })))
        }

        fn shared_with_gpt_partition() -> SharedBlockDevice {
            let mut bytes = vec![0u8; BLOCK_SIZE * 32];
            bytes[510..512].copy_from_slice(&[0x55, 0xAA]);
            bytes[446 + 4] = 0xEE;
            bytes[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
            bytes[446 + 12..446 + 16].copy_from_slice(&31u32.to_le_bytes());
            let entries = &mut bytes[BLOCK_SIZE * 2..BLOCK_SIZE * 3];
            entries[0] = 1;
            entries[32..40].copy_from_slice(&4u64.to_le_bytes());
            entries[40..48].copy_from_slice(&7u64.to_le_bytes());
            let mut entries_crc = 0xFFFF_FFFFu32;
            for byte in entries {
                entries_crc ^= *byte as u32;
                for _ in 0..8 {
                    entries_crc = if entries_crc & 1 != 0 {
                        (entries_crc >> 1) ^ 0xEDB8_8320
                    } else {
                        entries_crc >> 1
                    };
                }
            }
            entries_crc = !entries_crc;
            let header = &mut bytes[BLOCK_SIZE..BLOCK_SIZE * 2];
            header[..8].copy_from_slice(b"EFI PART");
            header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
            header[12..16].copy_from_slice(&92u32.to_le_bytes());
            header[24..32].copy_from_slice(&1u64.to_le_bytes());
            header[32..40].copy_from_slice(&31u64.to_le_bytes());
            header[40..48].copy_from_slice(&4u64.to_le_bytes());
            header[48..56].copy_from_slice(&28u64.to_le_bytes());
            header[72..80].copy_from_slice(&2u64.to_le_bytes());
            header[80..84].copy_from_slice(&4u32.to_le_bytes());
            header[84..88].copy_from_slice(&128u32.to_le_bytes());
            header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
            let mut header_crc = 0xFFFF_FFFFu32;
            for (index, byte) in header[..92].iter().enumerate() {
                let byte = if (16..20).contains(&index) { 0 } else { *byte };
                header_crc ^= byte as u32;
                for _ in 0..8 {
                    header_crc = if header_crc & 1 != 0 {
                        (header_crc >> 1) ^ 0xEDB8_8320
                    } else {
                        header_crc >> 1
                    };
                }
            }
            header[16..20].copy_from_slice(&(!header_crc).to_le_bytes());
            Arc::new(Mutex::new(Box::new(Self { bytes })))
        }
    }

    impl BlockDevice for RegistryDisk {
        fn total_blocks(&self) -> Option<u64> { Some((self.bytes.len() / BLOCK_SIZE) as u64) }

        fn read_blocks(&mut self, start : Lba, buf : &mut [u8]) -> DriverResult<()> {
            let start = usize::try_from(start.0)
                              .map_err(|_| DriverError::InvalidParam)?
                              .checked_mul(BLOCK_SIZE)
                              .ok_or(DriverError::InvalidParam)?;
            let source = self.bytes.get(start..start + buf.len())
                                   .ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(source);
            Ok(())
        }

        fn write_blocks(&mut self, _start : Lba, _buf : &[u8]) -> DriverResult<()> {
            Err(DriverError::Unsupported)
        }
    }

    #[test]
    fn unregister_disk_removes_children_without_reusing_slots() {
        let before_generation = device_topology_generation();
        let disk_index = register_block_device(RegistryDisk::shared_with_partition());
        let snapshot = block_devices_snapshot();
        let partition_index = snapshot.iter()
                                          .find_map(|(index, _, role)| {
                                              matches!(role,
                                                       BlockDeviceRole::Partition {
                                                           parent_device_index,
                                                           partition_number : 1,
                                                       } if *parent_device_index == disk_index)
                                                  .then_some(*index)
                                          })
                                          .expect("partition should be registered");
        assert!(device_topology_generation() > before_generation);
        assert!(unregister_block_device(disk_index));
        assert!(block_device_at(disk_index).is_none());
        assert!(block_device_at(partition_index).is_none());
        assert!(!unregister_block_device(disk_index));

        let next_index = register_block_device(RegistryDisk::shared_with_partition());
        assert!(next_index > partition_index, "stable slots must not be reused");
        assert!(unregister_block_device(next_index));
    }

    #[test]
    fn protective_mbr_publishes_gpt_partition_role() {
        let disk_index = register_block_device(RegistryDisk::shared_with_gpt_partition());
        let snapshot = block_devices_snapshot();
        assert!(snapshot.iter().any(|(_, _, role)| {
            matches!(role,
                     BlockDeviceRole::Partition { parent_device_index, partition_number : 1 }
                     if *parent_device_index == disk_index)
        }));
        assert!(unregister_block_device(disk_index));
    }
}
