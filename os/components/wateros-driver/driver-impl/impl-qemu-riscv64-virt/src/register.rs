//! 实例化并注册各子系统设备（transport: virtio-mmio / DTB UART）。

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use api_v0::{DeviceInfo, DeviceType, MmioRegion};
use block::{block_subsystem_claims_device, register_block_device, VirtioBlkDevice};
#[cfg(feature = "block-cache")]
use block::BlockCacheManager;
use network::{
    network_subsystem_claims_device, register_network_device, NetworkDevice, VirtioNetDevice,
};
#[cfg(feature = "display")]
use display::{
    display_subsystem_claims_device, register_display_device, DisplayDevice, VirtioGpuMmioDevice,
};
#[cfg(feature = "input")]
use input::{
    input_subsystem_claims_device, register_input_device, InputDevice, VirtioInputMmioDevice,
};
use character::{
    character_device_count, character_subsystem_claims_device, register_builtin_character_devices,
};
use common::dtb::is_virtio_mmio_compatible;
use spin::Mutex;

use crate::{enumerate, uart};

/// 成功注册为 virtio-blk 的 MMIO 窗口列表（供自检读取块 0）。
pub(crate) static VIRTIO_BLK_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());
/// 成功注册为 virtio-net 的 MMIO 窗口列表。
pub(crate) static VIRTIO_NET_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());
#[cfg(feature = "display")]
pub(crate) static VIRTIO_GPU_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());

/// 在已扫描的 `DEVICE_INFOS` 上尝试实例化 virtio-blk 与 virtio-net；失败或未声明的路径记入列表供 devfs 标注。
pub(crate) fn probe_virtio_devices() -> Vec<String> {
    let infos_snapshot: Vec<DeviceInfo> = enumerate::DEVICE_INFOS.lock().clone();
    VIRTIO_BLK_MMIO.lock().clear();
    VIRTIO_NET_MMIO.lock().clear();
    #[cfg(feature = "display")]
    VIRTIO_GPU_MMIO.lock().clear();

    let mut unsupported = Vec::new();
    let mut blk_regions = Vec::new();
    let mut net_regions = Vec::new();
    #[cfg(feature = "display")]
    let mut gpu_regions = Vec::new();

    for info in infos_snapshot.iter() {
        if !is_virtio_mmio_compatible(&info.compatibles) {
            continue;
        }

        let path = enumerate::sys_dev_path_for_dtb_node(&info.node_name);

        let Some(mmio) = info.mmio else {
            unsupported.push(path);
            continue;
        };

        let claimed_by_block = block_subsystem_claims_device(&info.compatibles, info.device_type);
        let claimed_by_network =
            network_subsystem_claims_device(&info.compatibles, info.device_type);
        #[cfg(feature = "display")]
        let claimed_by_display =
            display_subsystem_claims_device(&info.compatibles, info.device_type);
        #[cfg(feature = "input")]
        let claimed_by_input = input_subsystem_claims_device(&info.compatibles, info.device_type);

        let mut handled = false;
        if claimed_by_block && info.device_type == DeviceType::Block {
            handled = true;
            let device = match info.irq {
                Some(irq) => VirtioBlkDevice::from_mmio_with_irq(mmio, irq.irq),
                None => VirtioBlkDevice::from_mmio(mmio),
            };
            match device {
                Ok(dev) => {
                    let shared = {
                        #[cfg(feature = "block-cache")]
                        {
                            BlockCacheManager::wrap(
                                Arc::new(dev),
                                BlockCacheManager::default_config(),
                            )
                        }
                        #[cfg(not(feature = "block-cache"))]
                        {
                            Arc::new(dev)
                        }
                    };
                    let idx = register_block_device(shared);
                    blk_regions.push(mmio);
                    log::info!("[driver] registered virtio-blk #{}", idx);
                    log::info!(
                        "[driver] found virtio-blk: node={} base={:#x} size={:#x}",
                        info.node_name,
                        mmio.base,
                        mmio.size
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[driver] failed to init virtio-blk at base={:#x}: {:?}",
                        mmio.base,
                        err
                    );
                    unsupported.push(path.clone());
                }
            }
        } else if claimed_by_network && info.device_type == DeviceType::Network {
            handled = true;
            match VirtioNetDevice::from_mmio(mmio) {
                Ok(dev) => {
                    let mac = dev.mac_address();
                    let idx = register_network_device(Arc::new(Mutex::new(Box::new(dev))));
                    net_regions.push(mmio);
                    log::info!("[driver] registered virtio-net #{}", idx);
                    log::info!(
                        "[driver] found virtio-net: node={} mac={:02x?} base={:#x} size={:#x}",
                        info.node_name,
                        mac,
                        mmio.base,
                        mmio.size
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[driver] failed to init virtio-net at base={:#x}: {:?}",
                        mmio.base,
                        err
                    );
                    unsupported.push(path.clone());
                }
            }
        }
        #[cfg(feature = "display")]
        if !handled && claimed_by_display && info.device_type == DeviceType::Display {
            handled = true;
            match VirtioGpuMmioDevice::from_mmio(mmio) {
                Ok(device) => {
                    let framebuffer = device.info();
                    let device: Box<dyn DisplayDevice> = Box::new(device);
                    let idx = register_display_device(Arc::new(Mutex::new(device)));
                    gpu_regions.push(mmio);
                    log::info!(
                        "[driver] registered virtio-gpu #{} resolution={}x{} stride={}",
                        idx, framebuffer.width, framebuffer.height, framebuffer.stride
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[driver] failed to init virtio-gpu at base={:#x}: {:?}",
                        mmio.base, err
                    );
                    unsupported.push(path.clone());
                }
            }
        }
        #[cfg(feature = "input")]
        if !handled && claimed_by_input && info.device_type == DeviceType::Input {
            handled = true;
            match VirtioInputMmioDevice::from_mmio(mmio) {
                Ok(device) => {
                    let metadata = device.info().clone();
                    let device: Box<dyn InputDevice> = Box::new(device);
                    let idx = register_input_device(Arc::new(Mutex::new(device)));
                    log::info!("[driver] registered virtio-input #{} kind={:?} name={}",
                               idx, metadata.kind, metadata.name);
                }
                Err(err) => {
                    log::warn!("[driver] failed to init virtio-input at base={:#x}: {:?}",
                               mmio.base, err);
                    unsupported.push(path.clone());
                }
            }
        }
        if !handled {
            unsupported.push(path);
        }
    }
    *VIRTIO_BLK_MMIO.lock() = blk_regions;
    *VIRTIO_NET_MMIO.lock() = net_regions;
    #[cfg(feature = "display")]
    {
        *VIRTIO_GPU_MMIO.lock() = gpu_regions;
    }
    unsupported
}

/// 绑定 DTB 中的 UART 字符设备；若无匹配则回退到 QEMU virt 默认 UART0。
pub(crate) fn probe_character_devices() {
    let uart_bases: Vec<usize> = {
        let infos = enumerate::DEVICE_INFOS.lock();
        infos
            .iter()
            .filter(|info| {
                character_subsystem_claims_device(&info.compatibles, info.device_type)
            })
            .filter_map(|info| {
                if let Some(mmio) = info.mmio {
                    Some((mmio.base, info.node_name.clone()))
                } else {
                    log::warn!(
                        "[driver] dtb uart: node={} has no MMIO region",
                        info.node_name
                    );
                    None
                }
            })
            .map(|(base, _)| base)
            .collect()
    };

    for (idx, base) in uart_bases.iter().enumerate() {
        let chr_idx = uart::register_uart_character_device(*base);
        log::info!(
            "[driver] registered character #{} (uart base={:#x}, dtb #{})",
            chr_idx,
            base,
            idx
        );
    }

    if character_device_count() == 0 {
        let idx = uart::register_uart_character_device(uart::QEMU_VIRT_UART0_BASE);
        log::info!(
            "[driver] registered character #{} (fallback virt uart0 base={:#x})",
            idx,
            uart::QEMU_VIRT_UART0_BASE
        );
    }

    register_builtin_character_devices();
    log::info!(
        "[driver] character devices registered: count={}",
        character_device_count()
    );
}
