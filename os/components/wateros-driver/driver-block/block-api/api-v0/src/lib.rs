//! 块设备抽象：逻辑块寻址、全局注册表与默认块大小常量。
//!
//! [`BlockDevice`] 提供按块与按字节读取的默认实现；写路径由具体设备决定是否支持。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

pub mod partition;
pub use partition::{MbrPartition, PartitionBlockDevice, PartitionScanError, scan_mbr};

pub use driver_api::{DriverError, DriverResult};

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
    Partition { parent_device_index : usize, partition_number : u8 },
}

struct RegisteredBlockDevice {
    device : SharedBlockDevice,
    role : BlockDeviceRole,
}

static BLOCK_DEVICES : Mutex<Vec<RegisteredBlockDevice>> = Mutex::new(Vec::new());

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
    let mut devices = BLOCK_DEVICES.lock();
    let disk_number = devices.iter()
                             .filter(|entry| matches!(entry.role, BlockDeviceRole::Disk { .. }))
                             .count();
    devices.push(RegisteredBlockDevice { device : device.clone(),
                                         role : BlockDeviceRole::Disk { disk_number } });
    let disk_index = devices.len() - 1;
    drop(devices);

    match scan_mbr(&device) {
        Ok(partitions) => for partition in partitions {
            if let Ok(child) = PartitionBlockDevice::shared(device.clone(),
                                                            partition.start_lba,
                                                            partition.sectors)
            {
                register_partition_device(disk_index, partition.number, child);
            }
        },
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

fn register_partition_device(parent_device_index : usize,
                             partition_number : u8,
                             device : SharedBlockDevice) -> usize {
    let mut devices = BLOCK_DEVICES.lock();
    devices.push(RegisteredBlockDevice { device,
                                         role : BlockDeviceRole::Partition {
                                             parent_device_index,
                                             partition_number,
                                         } });
    devices.len() - 1
}

/// 当前已注册块设备数量，包括整盘与自动发现的分区设备。
pub fn block_device_count() -> usize {
    BLOCK_DEVICES.lock().len()
}

/// 取表中第一个设备，常用于根文件系统绑定单盘场景。
pub fn first_block_device() -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock().first().map(|entry| entry.device.clone())
}

/// 按下标取设备；越界返回 `None`。
pub fn block_device_at(index: usize) -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock().get(index).map(|entry| entry.device.clone())
}

pub fn block_device_role_at(index : usize) -> Option<BlockDeviceRole> {
    BLOCK_DEVICES.lock().get(index).map(|entry| entry.role)
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
