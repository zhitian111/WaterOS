//! Pure-data Loongson-2 APBDMA descriptor planning.
//!
//! This module performs no allocation, address translation, cache maintenance
//! or MMIO. A future executor must supply DMA-capable physical memory and the
//! architecture-specific cache operations before using a plan on hardware.
use crate::topology::DmaControllerDescription;
use alloc::vec::Vec;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLease {
    provider_phandle : u32,
    channel : u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    UnknownProvider,
    InvalidChannel,
    Busy,
    NotOwned,
}

pub struct ChannelLeases {
    slots : Vec<(u32, bool)>,
}

impl ChannelLeases {
    pub fn from_topology(controllers : &[DmaControllerDescription]) -> Self {
        Self { slots : controllers.iter().map(|controller| (controller.phandle, false)).collect() }
    }

    pub fn claim(&mut self, provider_phandle : u32, channel : u32)
                 -> Result<ChannelLease, LeaseError> {
        if channel != 0 { return Err(LeaseError::InvalidChannel); }
        let slot = self.slots.iter_mut()
                             .find(|(provider, _)| *provider == provider_phandle)
                             .ok_or(LeaseError::UnknownProvider)?;
        if slot.1 { return Err(LeaseError::Busy); }
        slot.1 = true;
        Ok(ChannelLease { provider_phandle, channel })
    }

    pub fn release(&mut self, lease : ChannelLease) -> Result<(), LeaseError> {
        let slot = self.slots.iter_mut()
                             .find(|(provider, _)| *provider == lease.provider_phandle)
                             .ok_or(LeaseError::UnknownProvider)?;
        if lease.channel != 0 || !slot.1 { return Err(LeaseError::NotOwned); }
        slot.1 = false;
        Ok(())
    }
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

pub struct PreparedTransfer(TransferPlan);

impl PreparedTransfer {
    /// # Safety
    /// The caller must have written `plan.descriptor` to the physical address,
    /// cleaned the descriptor cache lines, and for memory-to-device transfers
    /// cleaned the buffer cache lines before constructing this token.
    pub unsafe fn after_cache_sync(plan : TransferPlan) -> Self { Self(plan) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorError {
    Busy,
    Idle,
    Register,
}

pub trait OrderIo {
    fn read64(&mut self) -> Result<u64, ExecutorError>;
    fn write64(&mut self, value : u64) -> Result<(), ExecutorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    pub invalidate_buffer_after : bool,
}

pub struct Executor<R> {
    registers : R,
    running : Option<TransferPlan>,
}

impl<R : OrderIo> Executor<R> {
    pub fn new(registers : R) -> Self { Self { registers, running : None } }

    pub fn start(&mut self, prepared : PreparedTransfer) -> Result<(), ExecutorError> {
        if self.running.is_some() { return Err(ExecutorError::Busy); }
        self.registers.write64(0)?;
        self.registers.write64(prepared.0.start_order)?;
        self.running = Some(prepared.0);
        Ok(())
    }

    /// Called only after the APBDMA IRQ has been claimed and acknowledged.
    pub fn complete_irq(&mut self) -> Result<Completion, ExecutorError> {
        let plan = self.running.take().ok_or(ExecutorError::Idle)?;
        Ok(Completion { invalidate_buffer_after : plan.invalidate_buffer_after })
    }

    pub fn stop(&mut self) -> Result<(), ExecutorError> {
        if self.running.is_none() { return Err(ExecutorError::Idle); }
        let current = self.registers.read64()?;
        self.registers.write64((current & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4))?;
        self.running = None;
        Ok(())
    }

    #[cfg(test)]
    fn into_inner(self) -> R { self.registers }
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
    use alloc::vec;

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

    #[test]
    fn leases_one_channel_per_provider_without_reuse_while_busy() {
        use crate::topology::{DmaControllerDescription, InterruptSpec, NamedResource,
                              ResourceSpecifier};
        use api_v0::MmioRegion;
        let controller = DmaControllerDescription {
            phandle : 7,
            mmio : MmioRegion { base : 0x1fe0_0c10, size : 8 },
            interrupt : InterruptSpec { parent_phandle : 1,
                                        cells : [13, 4, 0, 0],
                                        cell_count : 2 },
            clock : NamedResource { name : None,
                                    specifier : ResourceSpecifier { provider_phandle : 2,
                                                                    args : vec![0] } },
            channel_cells : 1,
        };
        let mut leases = ChannelLeases::from_topology(&[controller]);
        let lease = leases.claim(7, 0).unwrap();
        assert_eq!(leases.claim(7, 0), Err(LeaseError::Busy));
        assert_eq!(leases.claim(7, 1), Err(LeaseError::InvalidChannel));
        assert_eq!(leases.claim(8, 0), Err(LeaseError::UnknownProvider));
        leases.release(lease).unwrap();
        assert!(leases.claim(7, 0).is_ok());
    }

    #[derive(Default)]
    struct MockOrderIo {
        value : u64,
        writes : Vec<u64>,
    }
    impl OrderIo for MockOrderIo {
        fn read64(&mut self) -> Result<u64, ExecutorError> { Ok(self.value) }
        fn write64(&mut self, value : u64) -> Result<(), ExecutorError> {
            self.value = value;
            self.writes.push(value);
            Ok(())
        }
    }

    #[test]
    fn executor_requires_prepared_token_and_tracks_completion() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        // SAFETY: mock memory is coherent and the test does not start hardware.
        let prepared = unsafe { PreparedTransfer::after_cache_sync(plan) };
        let mut executor = Executor::new(MockOrderIo::default());
        executor.start(prepared).unwrap();
        assert_eq!(executor.complete_irq(),
                   Ok(Completion { invalidate_buffer_after : true }));
        assert_eq!(executor.complete_irq(), Err(ExecutorError::Idle));
        let io = executor.into_inner();
        assert_eq!(io.writes, vec![0, plan.start_order]);
    }

    #[test]
    fn executor_rejects_overlap_and_encodes_stop() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let mut executor = Executor::new(MockOrderIo::default());
        // SAFETY: mock memory is coherent and the test does not start hardware.
        executor.start(unsafe { PreparedTransfer::after_cache_sync(plan) }).unwrap();
        assert_eq!(executor.start(unsafe { PreparedTransfer::after_cache_sync(plan) }),
                   Err(ExecutorError::Busy));
        executor.stop().unwrap();
        assert_eq!(executor.into_inner().writes.last().copied(),
                   Some((plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)));
    }
}
