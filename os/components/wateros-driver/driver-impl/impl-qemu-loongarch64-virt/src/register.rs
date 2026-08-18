//! 实例化并注册各子系统设备（transport: virtio-pci）。

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use api_v0::DriverResult;
use block::{register_block_device, BlockDevice, VirtioPciProbeInfo};
#[cfg(feature = "block-cache")]
use block::BlockCacheManager;
use network::{
    register_network_device, NetworkDevice, VirtioNetPciProbeInfo,
};
#[cfg(feature = "display")]
use display::{
    register_display_device, DisplayDevice, VirtioGpuPciProbeInfo,
};
#[cfg(feature = "input")]
use input::{
    register_input_device, InputDevice, VirtioInputPciProbeInfo,
};
use character::register_builtin_character_devices;
use spin::Mutex;

use crate::{enumerate, uart};

/// 成功注册为 virtio-blk 的 PCI 设备列表。
pub(crate) static VIRTIO_BLK_PCI: Mutex<Vec<VirtioPciProbeInfo>> = Mutex::new(Vec::new());
/// 成功注册为 virtio-net 的 PCI 设备列表。
pub(crate) static VIRTIO_NET_PCI: Mutex<Vec<VirtioNetPciProbeInfo>> = Mutex::new(Vec::new());
#[cfg(feature = "display")]
pub(crate) static VIRTIO_GPU_PCI: Mutex<Vec<VirtioGpuPciProbeInfo>> = Mutex::new(Vec::new());
#[cfg(feature = "input")]
pub(crate) static VIRTIO_INPUT_PCI: Mutex<Vec<VirtioInputPciProbeInfo>> = Mutex::new(Vec::new());

/// 枚举 PCIe ECAM 并注册 virtio-blk/net（以及可选的 gpu/input）与平台字符设备。
pub(crate) fn register_devices() -> DriverResult<()> {
    // 每次 bring-up 先清空本 profile 的探测快照，避免失败重试把旧设备重复计数。
    let mut blk = VIRTIO_BLK_PCI.lock();
    blk.clear();
    drop(blk);
    VIRTIO_NET_PCI.lock().clear();
    #[cfg(feature = "display")]
    VIRTIO_GPU_PCI.lock().clear();
    #[cfg(feature = "input")]
    VIRTIO_INPUT_PCI.lock().clear();

    // 尝试 PCIe ECAM 枚举 virtio-blk 设备。
    let config_base = enumerate::find_config_base();
    log::info!(
        "[driver-la] PCI config base = {:#x}",
        config_base
    );
    match enumerate::probe_virtio_blk_pci(config_base) {
        Ok(Some((dev, info))) => {
            let shared = {
                #[cfg(feature = "block-cache")]
                {
                    BlockCacheManager::wrap(
                        Box::new(dev),
                        BlockCacheManager::default_config(),
                    )
                }
                #[cfg(not(feature = "block-cache"))]
                {
                    let dev: Box<dyn BlockDevice> = Box::new(dev);
                    Arc::new(Mutex::new(dev))
                }
            };
            let idx = register_block_device(shared);
            VIRTIO_BLK_PCI.lock().push(info);
            log::info!(
                "[driver-la] registered virtio-blk #{} via PCI {}:{}.{} vendor={:#06x} \
                            device={:#06x}",
                idx,
                info.bus,
                info.device,
                info.function,
                info.vendor_id,
                info.device_id
            );
        }
        Ok(None) => {
            log::warn!("[driver-la][pci] no virtio-blk device found on PCI bus 0");
        }
        Err(err) => {
            log::warn!(
                "[driver-la] failed to init virtio-blk via PCI: {:?}",
                err
            );
        }
    }

    match enumerate::probe_virtio_net_pci(config_base) {
        Ok(Some((dev, info))) => {
            let mac = dev.mac_address();
            let shared = {
                let dev: Box<dyn NetworkDevice> = Box::new(dev);
                Arc::new(Mutex::new(dev))
            };
            let idx = register_network_device(shared);
            VIRTIO_NET_PCI.lock().push(info);
            log::info!(
                "[driver-la] registered virtio-net #{} via PCI {}:{}.{} vendor={:#06x} \
                            device={:#06x}",
                idx,
                info.bus,
                info.device,
                info.function,
                info.vendor_id,
                info.device_id
            );
            log::info!("[driver-la] found virtio-net-pci mac={:02x?}", mac);
        }
        Ok(None) => {
            log::warn!("[driver-la][pci] no virtio-net device found on PCI bus 0");
        }
        Err(err) => {
            log::warn!(
                "[driver-la] failed to init virtio-net via PCI: {:?}",
                err
            );
        }
    }

    #[cfg(feature = "display")]
    match enumerate::probe_virtio_gpu_pci(config_base) {
        Ok(Some((device, info))) => {
            let framebuffer = device.info();
            let device: Box<dyn DisplayDevice> = Box::new(device);
            let idx = register_display_device(Arc::new(Mutex::new(device)));
            VIRTIO_GPU_PCI.lock().push(info);
            log::info!(
                "[driver-la] registered virtio-gpu #{} via PCI {}:{}.{} resolution={}x{} stride={}",
                idx,
                info.bus,
                info.device,
                info.function,
                framebuffer.width,
                framebuffer.height,
                framebuffer.stride
            );
        }
        Ok(None) => {
            log::warn!("[driver-la][pci] no virtio-gpu device found on PCI bus 0");
        }
        Err(err) => {
            log::warn!("[driver-la] failed to init virtio-gpu via PCI: {:?}", err);
        }
    }

    #[cfg(feature = "input")]
    match enumerate::probe_virtio_input_pci(config_base) {
        Ok(devices) => {
            for (device, info) in devices {
                let device_info = device.info().clone();
                let device: Box<dyn InputDevice> = Box::new(device);
                let idx = register_input_device(Arc::new(Mutex::new(device)));
                VIRTIO_INPUT_PCI.lock().push(info);
                log::info!(
                    "[driver-la] registered virtio-input #{} via PCI {}:{}.{} name={} kind={:?}",
                    idx, info.bus, info.device, info.function, device_info.name, device_info.kind
                );
            }
        }
        Err(err) => {
            log::warn!("[driver-la] failed to init virtio-input via PCI: {:?}", err);
        }
    }

    register_builtin_character_devices();
    uart::register_uart_character_device();
    Ok(())
}
