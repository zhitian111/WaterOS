//! 块设备抽象：逻辑块寻址、全局注册表、MBR/GPT 分区发现与默认块大小常量。
//!
//! [`BlockDevice`] 提供按块与按字节读取的默认实现；写路径由具体设备决定是否支持。
//! 整盘注册时若首扇区包含可解析的分区表，对应分区设备会以
//! [`BlockDeviceRole::Partition`] 紧随整盘注册，供 devfs 暴露 `/dev/vdaN` 等路径。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

pub mod partition;
pub use partition::{GptPartition, MbrPartition, PartitionBlockDevice, PartitionScanError, scan_gpt,
                    scan_mbr};

pub use driver_api::{DriverError, DriverResult};

/// 逻辑块字节长度；当前 WaterOS bring-up 固定为 512（与 virtio-blk 常见配置一致）。
pub const BLOCK_SIZE : usize = 512;

/// 逻辑块地址（LBA），从 0 起算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lba(pub u64);

impl From<usize> for Lba {
    /// 将 `usize` 截断/拓宽为 `u64` LBA（与平台指针宽度一致的内核路径常用）。
    fn from(value : usize) -> Self {
        Self(value as u64)
    }
}

impl From<u64> for Lba {
    /// 直接包装为 LBA，无额外校验（非法 LBA 由具体设备在读时拒绝）。
    fn from(value : u64) -> Self {
        Self(value)
    }
}

/// 可在多任务间共享的块设备句柄（内部可变性由 `spin::Mutex` 提供）。
pub type SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>;

/// 块设备在注册表中的语义角色：整盘或整盘的分区视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockDeviceRole {
    /// 整盘；`disk_number` 为 Linux 风格盘号（vda/vdb…）。
    Disk { disk_number : usize },
    /// 整盘上的分区视图；`parent_device_index` 为整盘在注册表中的下标。
    Partition { parent_device_index : usize, partition_number : u32 },
}

struct RegisteredBlockDevice {
    device : SharedBlockDevice,
    role : BlockDeviceRole,
}

/// 注册顺序稳定：`register_block_device` 返回整盘下标，分区紧随其后。
static BLOCK_DEVICES : Mutex<Vec<Option<RegisteredBlockDevice>>> = Mutex::new(Vec::new());

/// 块设备语义契约：按块读写为必须实现；按字节读提供默认实现（内部临时缓冲整段块）。
pub trait BlockDevice: Send {
    /// 设备逻辑块大小；默认 [`BLOCK_SIZE`]。
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    /// 设备总块数；未知时返回 `None`（默认）。
    fn total_blocks(&self) -> Option<u64> {
        None
    }

    /// Validate a whole-block request before it reaches a device or cache.
    ///
    /// This rejects zero-sized logical blocks, partial buffers, LBA arithmetic
    /// overflow, and requests beyond a reported device capacity.  Devices with
    /// an unknown capacity still get the alignment and overflow checks.
    fn check_request_range(&self, start_block : Lba, byte_len : usize) -> DriverResult<()> {
        let block_size = self.block_size();
        if block_size == 0 || byte_len % block_size != 0 {
            return Err(DriverError::InvalidParam);
        }
        let count = u64::try_from(byte_len / block_size).map_err(|_| DriverError::InvalidParam)?;
        let end = start_block.0
                             .checked_add(count)
                             .ok_or(DriverError::InvalidParam)?;
        if self.total_blocks()
               .is_some_and(|capacity| end > capacity)
        {
            return Err(DriverError::InvalidParam);
        }
        Ok(())
    }

    /// 从 `start_block` 起读取连续块到 `buf`；`buf` 长度须为块大小的整数倍。
    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()>;

    /// 从 `start_block` 起写入连续块；不支持写时须返回 [`DriverError::Unsupported`]。
    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()>;

    /// Commit all previously accepted writes to stable storage.
    fn flush(&mut self) -> DriverResult<()>;

    /// 任意字节对齐读取：通过整段块缓冲实现，调用方须保证 `dst` 非空且偏移合法。
    fn read_bytes(&mut self, offset : u64, dst : &mut [u8]) -> DriverResult<()> {
        if dst.is_empty() {
            return Ok(());
        }

        let block_size = self.block_size();
        if block_size == 0 {
            return Err(DriverError::InvalidParam);
        }

        // `offset` 须可落入 `usize`：内核 bring-up 路径通常远低于平台地址空间上限。
        let start_byte = usize::try_from(offset).map_err(|_| DriverError::InvalidParam)?;
        let end_byte = start_byte.checked_add(dst.len())
                                 .ok_or(DriverError::InvalidParam)?;
        let start_block = start_byte / block_size;
        let end_block = end_byte.div_ceil(block_size);
        let block_count = end_block.checked_sub(start_block)
                                   .ok_or(DriverError::InvalidParam)?;
        let scratch_len = block_count.checked_mul(block_size)
                                     .ok_or(DriverError::InvalidParam)?;
        // 临时覆盖跨块区间；设备侧仍只看到整倍数 `block_size` 的 `read_blocks` 调用。
        let mut scratch = vec![0u8; scratch_len];

        self.read_blocks(Lba(start_block as u64), &mut scratch)?;

        let offset_in_block = start_byte % block_size;
        let read_end = offset_in_block.checked_add(dst.len())
                                      .ok_or(DriverError::InvalidParam)?;
        dst.copy_from_slice(&scratch[offset_in_block..read_end]);
        Ok(())
    }

    /// 读取从 `offset` 起的 `len` 字节到新分配的缓冲区。
    fn read_prefix(&mut self, offset : u64, len : usize) -> DriverResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read_bytes(offset, &mut buf)?;
        Ok(buf)
    }
}

/// 将整盘追加到全局表末尾，返回整盘下标（从 0 起）。
///
/// 若首扇区包含受支持的 MBR 主分区表（或 GPT 保护性 MBR），对应的有界分区设备
/// 会紧随整盘注册；没有分区表是合法的整盘文件系统布局。
pub fn register_block_device(device : SharedBlockDevice) -> usize {
    let scan = scan_mbr(&device);
    let mut children = Vec::new();
    let mut gpt_error = None;
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
            match scan_gpt(&device) {
                Ok(partitions) => {
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
                Err(error) => gpt_error = Some(error),
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

    match scan {
        Ok(_) => {}
        // 没有 MBR 签名是合法的整盘文件系统布局，不需要告警。
        Err(PartitionScanError::InvalidSignature) => {}
        // Protective MBR is expected when GPT was parsed successfully. Only
        // report the bounded GPT failure that prevented child registration.
        Err(PartitionScanError::ProtectiveGpt) if gpt_error.is_none() => {}
        Err(error) => {
            #[cfg(feature = "logging")]
            logging::warn!("[driver-block-api] disk #{disk_number} partition scan skipped: {error:?}");
            #[cfg(not(feature = "logging"))]
            let _ = error;
        }
    }
    if let Some(error) = gpt_error {
        #[cfg(feature = "logging")]
        logging::warn!("[driver-block-api] disk #{disk_number} GPT scan failed: {error:?}");
        #[cfg(not(feature = "logging"))]
        let _ = error;
    }
    disk_index
}

fn first_available_disk_number(devices : &[Option<RegisteredBlockDevice>]) -> usize {
    (0..).find(|candidate| {
             !devices.iter()
                     .flatten()
                     .any(|entry| {
                         matches!(entry.role,
                                  BlockDeviceRole::Disk { disk_number }
                                  if disk_number == *candidate)
                     })
         })
         .expect("finite registry always has a free disk number")
}

/// 当前已注册块设备数量，包括整盘与自动发现的分区设备。
pub fn block_device_count() -> usize {
    BLOCK_DEVICES.lock()
                 .iter()
                 .flatten()
                 .count()
}

/// 取表中第一个活动设备（整盘），常用于根文件系统绑定单盘场景。
pub fn first_block_device() -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock()
                 .iter()
                 .flatten()
                 .next()
                 .map(|entry| entry.device.clone())
}

/// 按下标取活动设备；越界或已注销返回 `None`。
pub fn block_device_at(index : usize) -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock()
                 .get(index)
                 .and_then(Option::as_ref)
                 .map(|entry| entry.device.clone())
}

/// 按下标取设备角色；越界或已注销返回 `None`。
pub fn block_device_role_at(index : usize) -> Option<BlockDeviceRole> {
    BLOCK_DEVICES.lock()
                 .get(index)
                 .and_then(Option::as_ref)
                 .map(|entry| entry.role)
}

/// 不持注册表锁地快照所有活动设备及其角色。
pub fn block_devices_snapshot() -> Vec<(usize, SharedBlockDevice, BlockDeviceRole)> {
    BLOCK_DEVICES.lock()
                 .iter()
                 .enumerate()
                 .filter_map(|(index, entry)| {
                     entry.as_ref()
                          .map(|entry| (index, entry.device.clone(), entry.role))
                 })
                 .collect()
}

/// 注销一个注册表槽位；注销整盘时连带移除其分区子设备。
///
/// 已存在的 [`SharedBlockDevice`] 克隆不受影响；物理热拔驱动必须先静默 DMA 并让
/// 在途 I/O 失败再调用，该硬件序列无法在无目标板环境下测试。
pub fn unregister_block_device(index : usize) -> bool {
    let mut devices = BLOCK_DEVICES.lock();
    let Some(role) = devices.get(index)
                            .and_then(Option::as_ref)
                            .map(|entry| entry.role)
    else {
        return false;
    };
    devices[index] = None;
    if matches!(role, BlockDeviceRole::Disk { .. }) {
        for entry in devices.iter_mut() {
            if entry.as_ref()
                    .is_some_and(|entry| {
                        matches!(entry.role,
                                 BlockDeviceRole::Partition { parent_device_index, .. }
                                 if parent_device_index == index)
                    })
            {
                *entry = None;
            }
        }
    }
    true
}

/// 自检：校验常量与样例设备的 [`read_prefix`] 行为。
pub fn test() {
    #[cfg(feature = "logging")]
    logging::trace!("[driver-block-api] test begin");
    assert_eq!(BLOCK_SIZE, 512);
    let mut sample = SampleBlockDevice::new();
    let prefix = sample.read_prefix(3, 5)
                       .expect("prefix read should work");
    assert_eq!(&prefix, &[3, 4, 5, 6, 7]);
    assert!(sample.check_request_range(Lba(1), BLOCK_SIZE).is_ok());
    assert_eq!(sample.check_request_range(Lba(2), BLOCK_SIZE),
               Err(DriverError::InvalidParam));
    assert_eq!(sample.check_request_range(Lba(u64::MAX), BLOCK_SIZE),
               Err(DriverError::InvalidParam));
    assert_eq!(sample.check_request_range(Lba(0), BLOCK_SIZE - 1),
               Err(DriverError::InvalidParam));
    sample.flush().expect("sample flush should work");
    #[cfg(feature = "logging")]
    logging::trace!("[driver-block-api] test end");
}

// 内存中的连续字节数组模拟两块设备；`read_blocks` 按字节偏移切片，写路径恒不支持。
struct SampleBlockDevice {
    bytes : [u8; BLOCK_SIZE * 2],
}

impl SampleBlockDevice {
    // 字节值等于下标 mod 256，便于 `read_prefix` 断言可读内容可预测。
    fn new() -> Self {
        let mut bytes = [0u8; BLOCK_SIZE * 2];
        for (idx, value) in bytes.iter_mut()
                                 .enumerate()
        {
            *value = idx as u8;
        }
        Self { bytes }
    }
}

impl BlockDevice for SampleBlockDevice {
    fn total_blocks(&self) -> Option<u64> {
        Some(2)
    }

    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        if buf.len() % BLOCK_SIZE != 0 {
            return Err(DriverError::InvalidParam);
        }
        let start = usize::try_from(start_block.0)
            .map_err(|_| DriverError::InvalidParam)?
            .checked_mul(BLOCK_SIZE)
            .ok_or(DriverError::InvalidParam)?;
        let end = start.checked_add(buf.len())
                       .ok_or(DriverError::InvalidParam)?;
        let src = self.bytes
                      .get(start..end)
                      .ok_or(DriverError::InvalidParam)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write_blocks(&mut self, _start_block : Lba, _buf : &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }

    fn flush(&mut self) -> DriverResult<()> {
        Ok(())
    }
}
