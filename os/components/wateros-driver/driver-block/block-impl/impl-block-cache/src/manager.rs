//! 本模块代码由AI完成
//! 块缓存注册辅助：从 [`wateros_base_config`] 读取默认容量并包装 [`CachingBlockDevice`]。

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use api_v0::{BlockDevice, SharedBlockDevice};
use spin::Mutex;
use wateros_base_config::fs::BLOCK_CACHE_CAPACITY_BLOCKS;

use crate::{BlockCacheConfig, CachingBlockDevice};

/// 块设备写穿缓存的管理入口（v1：包装与默认配置；不向下转型已注册句柄）。
// 本结构代码由AI完成
pub struct BlockCacheManager;

impl BlockCacheManager {
    /// 默认 [`BlockCacheConfig`]，容量来自 `base-config`。
    pub fn default_config() -> BlockCacheConfig {
        BlockCacheConfig { capacity_blocks : BLOCK_CACHE_CAPACITY_BLOCKS }
    }

    /// 用写穿 LRU 包装 `inner` 并返回可注册的共享句柄。
    pub fn wrap(inner : Box<dyn BlockDevice + Send>,
                config : BlockCacheConfig)
                -> SharedBlockDevice {
        let cached : Box<dyn BlockDevice> = Box::new(CachingBlockDevice::new(inner, config));
        Arc::new(Mutex::new(cached))
    }

    /// Flush every registered block device, including uncached devices.
    pub fn flush_all() -> api_v0::DriverResult<()> {
        for index in 0..api_v0::block_device_count() {
            let device = api_v0::block_device_at(index).ok_or(api_v0::DriverError::IoError)?;
            device.lock()
                  .flush()?;
        }
        Ok(())
    }

    /// 已注册块设备数量（与全局表一致，含非缓存设备）。
    pub fn registered_count() -> usize { api_v0::block_device_count() }
}
