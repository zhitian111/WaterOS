//! Pure-data Loongson-2 APBDMA descriptor planning.
//!
//! This module performs no allocation, address translation, cache maintenance
//! or MMIO. A future executor must supply DMA-capable physical memory and the
//! architecture-specific cache operations before using a plan on hardware.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    DeviceToMemory,
    MemoryToDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    EmptyTransfer,
    UnalignedLength,
    UnalignedDescriptor,
    InvalidBurst,
    AddressOutOfRange,
    ArithmeticOverflow,
}

/// Hardware layout used by the upstream Loongson-2 APBDMA driver.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HardwareDescriptor {
    pub next_address_low : u32,
    pub memory_address_low : u32,
    pub apb_address : u32,
    pub length_words : u32,
    pub step_length : u32,
    pub step_times : u32,
    pub command : u32,
    pub status : u32,
    pub next_address_high : u32,
    pub memory_address_high : u32,
    pub reserved : [u32; 2],
}

const COMMAND_INTERRUPT : u32 = 1 << 1;
const COMMAND_MEMORY_TO_DEVICE : u32 = 1 << 12;
const ORDER_64_BIT : u64 = 1 << 0;
const ORDER_START : u64 = 1 << 3;
const ORDER_CONFIG_MASK : u64 = 0x1f;
const DMA_ROUTE_MASK : u32 = 0b111 << 15;
const DMA1_ROUTE : u32 = 1 << 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferPlan {
    pub descriptor : HardwareDescriptor,
    pub descriptor_physical_address : u64,
    pub start_order : u64,
    /// The buffer must be invalidated after a device-to-memory transfer.
    pub invalidate_buffer_after : bool,
    /// Descriptor contents must be visible before the order register is written.
    pub clean_descriptor_before : bool,
}

/// Preserve unrelated system-register bits and route SDIO to APBDMA1.
pub const fn route_sdio_to_dma1(current : u32) -> u32 {
    (current & !DMA_ROUTE_MASK) | DMA1_ROUTE
}

pub fn build_transfer(descriptor_physical_address : u64,
                      memory_physical_address : u64,
                      apb_address : u64,
                      byte_length : usize,
                      burst_words : u32,
                      direction : Direction)
                      -> Result<TransferPlan, PlanError> {
    if byte_length == 0 { return Err(PlanError::EmptyTransfer); }
    if byte_length % 4 != 0 { return Err(PlanError::UnalignedLength); }
    if descriptor_physical_address & ORDER_CONFIG_MASK != 0 {
        return Err(PlanError::UnalignedDescriptor);
    }
    if burst_words == 0 { return Err(PlanError::InvalidBurst); }
    let apb_address = u32::try_from(apb_address).map_err(|_| PlanError::AddressOutOfRange)?;
    let words = u32::try_from(byte_length / 4).map_err(|_| PlanError::ArithmeticOverflow)?;
    let step_times = words.div_ceil(burst_words);
    let length_words = words.div_ceil(step_times);
    let command = COMMAND_INTERRUPT |
                  if direction == Direction::MemoryToDevice {
                      COMMAND_MEMORY_TO_DEVICE
                  } else {
                      0
                  };
    let descriptor = HardwareDescriptor {
        memory_address_low : memory_physical_address as u32,
        memory_address_high : (memory_physical_address >> 32) as u32,
        apb_address,
        length_words,
        step_times,
        command,
        ..HardwareDescriptor::default()
    };
    Ok(TransferPlan {
        descriptor,
        descriptor_physical_address,
        start_order : (descriptor_physical_address & !ORDER_CONFIG_MASK) |
                      ORDER_64_BIT | ORDER_START,
        invalidate_buffer_after : direction == Direction::DeviceToMemory,
        clean_descriptor_before : true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_64_bit_read_descriptor_and_start_order() {
        let plan = build_transfer(0x1_0000_0040,
                                  0x2_1234_5000,
                                  0x1fe2_c040,
                                  512,
                                  16,
                                  Direction::DeviceToMemory).unwrap();
        assert_eq!(core::mem::size_of::<HardwareDescriptor>(), 48);
        assert_eq!(plan.descriptor.memory_address_low, 0x1234_5000);
        assert_eq!(plan.descriptor.memory_address_high, 2);
        assert_eq!(plan.descriptor.apb_address, 0x1fe2_c040);
        assert_eq!(plan.descriptor.length_words, 16);
        assert_eq!(plan.descriptor.step_times, 8);
        assert_eq!(plan.descriptor.command, COMMAND_INTERRUPT);
        assert_eq!(plan.start_order, 0x1_0000_0049);
        assert!(plan.invalidate_buffer_after);
        assert!(plan.clean_descriptor_before);
    }

    #[test]
    fn encodes_write_direction_and_dma_route_without_clobbering_bits() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        assert_eq!(plan.descriptor.command,
                   COMMAND_INTERRUPT | COMMAND_MEMORY_TO_DEVICE);
        assert!(!plan.invalidate_buffer_after);
        assert_eq!(route_sdio_to_dma1(0xa5a5_7fff),
                   (0xa5a5_7fff & !DMA_ROUTE_MASK) | DMA1_ROUTE);
    }

    #[test]
    fn rejects_unsafe_or_unrepresentable_transfers() {
        assert_eq!(build_transfer(0x2000, 0, 0, 0, 1, Direction::DeviceToMemory),
                   Err(PlanError::EmptyTransfer));
        assert_eq!(build_transfer(0x2000, 0, 0, 3, 1, Direction::DeviceToMemory),
                   Err(PlanError::UnalignedLength));
        assert_eq!(build_transfer(0x2004, 0, 0, 4, 1, Direction::DeviceToMemory),
                   Err(PlanError::UnalignedDescriptor));
        assert_eq!(build_transfer(0x2000, 0, 0, 4, 0, Direction::DeviceToMemory),
                   Err(PlanError::InvalidBurst));
        assert_eq!(build_transfer(0x2000, 0, u32::MAX as u64 + 1, 4, 1,
                                  Direction::DeviceToMemory),
                   Err(PlanError::AddressOutOfRange));
    }
}
