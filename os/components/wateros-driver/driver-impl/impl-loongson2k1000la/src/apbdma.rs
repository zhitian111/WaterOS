//! Pure-data Loongson-2 APBDMA descriptor planning.
//!
//! This module performs no allocation, address translation, cache maintenance
//! or MMIO. A future executor must supply DMA-capable physical memory and the
//! architecture-specific cache operations before using a plan on hardware.
use crate::topology::DmaControllerDescription;
use crate::dma_memory::DmaAllocationError;
#[cfg(target_arch = "loongarch64")]
use crate::dma_memory::OwnedDmaBuffer;
use alloc::vec::Vec;
use api_v0::{DriverError, DriverResult,
             dma::{DmaCoherency, DmaDirection, DmaMapping}};

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
    pub memory_physical_address : u64,
    pub byte_length : usize,
    pub direction : Direction,
    /// The buffer must be invalidated after a device-to-memory transfer.
    pub invalidate_buffer_after : bool,
    /// Descriptor contents must be visible before the order register is written.
    pub clean_descriptor_before : bool,
}

#[derive(Debug)]
pub struct PreparedTransfer(TransferPlan);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    Allocation(DmaAllocationError),
    InvalidPlan(PlanError),
    Driver(DriverError),
}

impl From<DmaAllocationError> for ResourceError {
    fn from(error : DmaAllocationError) -> Self { Self::Allocation(error) }
}

impl From<PlanError> for ResourceError {
    fn from(error : PlanError) -> Self { Self::InvalidPlan(error) }
}

impl From<DriverError> for ResourceError {
    fn from(error : DriverError) -> Self { Self::Driver(error) }
}

/// Descriptor and payload allocations whose lifetime covers one APBDMA transfer.
#[cfg(target_arch = "loongarch64")]
pub struct OwnedTransferResources<D, P> {
    plan : TransferPlan,
    descriptor : OwnedDmaBuffer<D>,
    payload : OwnedDmaBuffer<P>,
}

#[cfg(target_arch = "loongarch64")]
impl<D : DmaCoherency, P : DmaCoherency> OwnedTransferResources<D, P> {
    /// Allocate real contiguous RAM and encode its physical addresses into the
    /// descriptor. Supplying a production coherency backend remains mandatory.
    pub fn allocate(apb_address : u64,
                    byte_length : usize,
                    burst_words : u32,
                    direction : Direction,
                    device_address_bits : u8,
                    descriptor_coherency : D,
                    payload_coherency : P)
                    -> Result<Self, ResourceError> {
        let mut descriptor = OwnedDmaBuffer::allocate_zeroed(
            core::mem::size_of::<HardwareDescriptor>(),
            32,
            device_address_bits,
            DmaDirection::ToDevice,
            descriptor_coherency)?;
        let payload = OwnedDmaBuffer::allocate_zeroed(byte_length,
                                                      4,
                                                      device_address_bits,
                                                      dma_direction(direction),
                                                      payload_coherency)?;
        let descriptor_region = descriptor.region()?;
        let payload_region = payload.region()?;
        let plan = build_transfer(descriptor_region.physical_address(),
                                  payload_region.physical_address(),
                                  apb_address,
                                  byte_length,
                                  burst_words,
                                  direction)?;
        let descriptor_bytes = unsafe {
            core::slice::from_raw_parts(
                (&plan.descriptor as *const HardwareDescriptor).cast::<u8>(),
                core::mem::size_of::<HardwareDescriptor>())
        };
        descriptor.cpu_bytes_mut()?.copy_from_slice(descriptor_bytes);
        Ok(Self { plan, descriptor, payload })
    }

    pub const fn plan(&self) -> TransferPlan { self.plan }

    pub fn payload_bytes(&self) -> DriverResult<&[u8]> { self.payload.cpu_bytes() }

    pub fn payload_bytes_mut(&mut self) -> DriverResult<&mut [u8]> {
        self.payload.cpu_bytes_mut()
    }

    pub fn prepare_session(&mut self) -> DriverResult<PreparedSession<'_, D, P>> {
        prepare_session(self.plan,
                        self.descriptor.mapping_mut(),
                        self.payload.mapping_mut())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorError {
    Busy,
    Idle,
    Register,
}

#[derive(Debug)]
pub struct StartFailure {
    pub error : ExecutorError,
    pub prepared : PreparedTransfer,
}

pub trait OrderIo {
    fn read64(&mut self) -> Result<u64, ExecutorError>;
    fn write64(&mut self, value : u64) -> Result<(), ExecutorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    invalidate_buffer_after : bool,
}

impl Completion {
    pub const fn requires_buffer_invalidate(self) -> bool { self.invalidate_buffer_after }
}

/// A failed state transition that returns the original session for retry or
/// explicit cancellation.
pub struct SessionFailure<E, S> {
    pub error : E,
    pub session : S,
}

impl<E : core::fmt::Debug, S> core::fmt::Debug for SessionFailure<E, S> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("SessionFailure")
                 .field("error", &self.error)
                 .finish_non_exhaustive()
    }
}

/// Both mappings have been synchronized for the device but hardware has not
/// started. The mutable borrows keep the backing resources alive.
#[must_use = "cancel or start a prepared DMA session"]
pub struct PreparedSession<'a, D, P> {
    prepared : PreparedTransfer,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// Hardware is running and exclusively borrows both mappings and the executor.
#[must_use = "complete the IRQ or stop a running DMA session"]
pub struct RunningSession<'a, 'e, R, D, P> {
    executor : &'e mut Executor<R>,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// Hardware is confirmed idle; CPU-side cache synchronization remains.
#[must_use = "finish cache synchronization for a quiesced DMA session"]
pub struct QuiescedSession<'a, D, P> {
    completion : Completion,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

impl<'a, D : DmaCoherency, P : DmaCoherency> PreparedSession<'a, D, P> {
    pub fn cancel(self) -> Result<(), SessionFailure<DriverError, Self>> {
        match cancel_prepared(&self.prepared, self.descriptor, self.payload) {
            Ok(()) => Ok(()),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }

    pub fn start<'e, R : OrderIo>(self,
                                  executor : &'e mut Executor<R>)
                                  -> Result<RunningSession<'a, 'e, R, D, P>,
                                            SessionFailure<ExecutorError, Self>> {
        match executor.start(self.prepared) {
            Ok(()) => Ok(RunningSession { executor,
                                         descriptor : self.descriptor,
                                         payload : self.payload }),
            Err(failure) => Err(SessionFailure {
                error : failure.error,
                session : PreparedSession { prepared : failure.prepared,
                                            descriptor : self.descriptor,
                                            payload : self.payload },
            }),
        }
    }
}

impl<'a, 'e, R : OrderIo, D, P> RunningSession<'a, 'e, R, D, P> {
    pub fn complete_irq(self)
                        -> Result<QuiescedSession<'a, D, P>,
                                  SessionFailure<ExecutorError, Self>> {
        match self.executor.complete_irq() {
            Ok(completion) => Ok(QuiescedSession { completion,
                                                  descriptor : self.descriptor,
                                                  payload : self.payload }),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }

    pub fn stop(self)
                -> Result<QuiescedSession<'a, D, P>,
                          SessionFailure<ExecutorError, Self>> {
        match self.executor.stop() {
            Ok(completion) => Ok(QuiescedSession { completion,
                                                  descriptor : self.descriptor,
                                                  payload : self.payload }),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }
}

impl<'a, D : DmaCoherency, P : DmaCoherency> QuiescedSession<'a, D, P> {
    pub fn finish(self) -> Result<(), SessionFailure<DriverError, Self>> {
        match finish_transfer(self.completion, self.descriptor, self.payload) {
            Ok(()) => Ok(()),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }
}

fn dma_direction(direction : Direction) -> DmaDirection {
    match direction {
        Direction::DeviceToMemory => DmaDirection::FromDevice,
        Direction::MemoryToDevice => DmaDirection::ToDevice,
    }
}

/// Validate actual mappings against the plan and transfer both to the device.
/// Safe code cannot construct [`PreparedTransfer`] by assertion alone.
pub(crate) fn prepare_transfer<D : DmaCoherency, P : DmaCoherency>(
    plan : TransferPlan,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<PreparedTransfer> {
    let descriptor_region = descriptor.cpu_region()?;
    let payload_region = payload.cpu_region()?;
    if descriptor.direction() != DmaDirection::ToDevice ||
       descriptor_region.physical_address() != plan.descriptor_physical_address ||
       descriptor_region.length() < core::mem::size_of::<HardwareDescriptor>() ||
       payload.direction() != dma_direction(plan.direction) ||
       payload_region.physical_address() != plan.memory_physical_address ||
       payload_region.length() != plan.byte_length
    {
        return Err(DriverError::InvalidParam);
    }
    descriptor.prepare_for_device()?;
    if let Err(error) = payload.prepare_for_device() {
        descriptor.reclaim_after_stop().map_err(|_| DriverError::IoError)?;
        return Err(error);
    }
    Ok(PreparedTransfer(plan))
}

pub fn prepare_session<'a, D : DmaCoherency, P : DmaCoherency>(
    plan : TransferPlan,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>)
    -> DriverResult<PreparedSession<'a, D, P>> {
    let prepared = prepare_transfer(plan, descriptor, payload)?;
    Ok(PreparedSession { prepared, descriptor, payload })
}

pub(crate) fn finish_transfer<D : DmaCoherency, P : DmaCoherency>(
    _completion : Completion,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<()> {
    let payload_result = if payload.is_cpu_owned() {
        Ok(())
    } else {
        payload.complete_from_device().map(|_| ())
    };
    let descriptor_result = if descriptor.is_cpu_owned() {
        Ok(())
    } else {
        descriptor.complete_from_device().map(|_| ())
    };
    if payload.is_cpu_owned() && descriptor.is_cpu_owned() { return Ok(()); }
    payload_result?;
    descriptor_result?;
    Ok(())
}

pub(crate) fn cancel_prepared<D : DmaCoherency, P : DmaCoherency>(
    _prepared : &PreparedTransfer,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<()> {
    let payload_result = if payload.is_cpu_owned() {
        Ok(())
    } else {
        payload.reclaim_after_stop().map(|_| ())
    };
    let descriptor_result = if descriptor.is_cpu_owned() {
        Ok(())
    } else {
        descriptor.reclaim_after_stop().map(|_| ())
    };
    if payload.is_cpu_owned() && descriptor.is_cpu_owned() { return Ok(()); }
    payload_result?;
    descriptor_result?;
    Ok(())
}

pub struct Executor<R> {
    registers : R,
    running : Option<TransferPlan>,
}

impl<R : OrderIo> Executor<R> {
    pub fn new(registers : R) -> Self { Self { registers, running : None } }

    pub fn start(&mut self, prepared : PreparedTransfer) -> Result<(), StartFailure> {
        if self.running.is_some() {
            return Err(StartFailure { error : ExecutorError::Busy, prepared });
        }
        if let Err(error) = self.registers.write64(0) {
            return Err(StartFailure { error, prepared });
        }
        if let Err(error) = self.registers.write64(prepared.0.start_order) {
            return Err(StartFailure { error, prepared });
        }
        self.running = Some(prepared.0);
        Ok(())
    }

    /// Called only after the APBDMA IRQ has been claimed and acknowledged.
    pub fn complete_irq(&mut self) -> Result<Completion, ExecutorError> {
        let plan = self.running.take().ok_or(ExecutorError::Idle)?;
        Ok(Completion { invalidate_buffer_after : plan.invalidate_buffer_after })
    }

    pub fn stop(&mut self) -> Result<Completion, ExecutorError> {
        let plan = self.running.ok_or(ExecutorError::Idle)?;
        let current = self.registers.read64()?;
        self.registers.write64((current & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4))?;
        self.running = None;
        Ok(Completion { invalidate_buffer_after : plan.invalidate_buffer_after })
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
        memory_physical_address,
        byte_length,
        direction,
        invalidate_buffer_after : direction == Direction::DeviceToMemory,
        clean_descriptor_before : true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use api_v0::dma::{DmaRegion, DmaCoherency};

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

    #[derive(Default)]
    struct MockCache {
        device_syncs : usize,
        cpu_syncs : usize,
        fail_device : bool,
        fail_cpu_syncs : usize,
    }
    impl DmaCoherency for MockCache {
        fn sync_for_device(&mut self, _region : DmaRegion, _direction : DmaDirection)
                           -> DriverResult<()> {
            if self.fail_device { return Err(DriverError::IoError); }
            self.device_syncs += 1;
            Ok(())
        }
        fn sync_for_cpu(&mut self, _region : DmaRegion, _direction : DmaDirection)
                        -> DriverResult<()> {
            if self.fail_cpu_syncs > 0 {
                self.fail_cpu_syncs -= 1;
                return Err(DriverError::IoError);
            }
            self.cpu_syncs += 1;
            Ok(())
        }
    }

    fn mappings(plan : TransferPlan) -> (DmaMapping<MockCache>, DmaMapping<MockCache>) {
        let descriptor = DmaRegion::new(0x4000,
                                        plan.descriptor_physical_address,
                                        64,
                                        32,
                                        64).unwrap();
        let payload = DmaRegion::new(0x8000,
                                     plan.memory_physical_address,
                                     plan.byte_length,
                                     32,
                                     64).unwrap();
        (DmaMapping::new(descriptor, DmaDirection::ToDevice, MockCache::default()),
         DmaMapping::new(payload, dma_direction(plan.direction), MockCache::default()))
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
        let (mut descriptor, mut payload) = mappings(plan);
        let prepared = prepare_transfer(plan, &mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default());
        assert!(executor.start(prepared).is_ok());
        let completion = executor.complete_irq().unwrap();
        assert!(completion.requires_buffer_invalidate());
        finish_transfer(completion, &mut descriptor, &mut payload).unwrap();
        assert!(descriptor.cpu_region().is_ok());
        assert!(payload.cpu_region().is_ok());
        assert_eq!(executor.complete_irq(), Err(ExecutorError::Idle));
        let io = executor.into_inner();
        assert_eq!(io.writes, vec![0, plan.start_order]);
    }

    #[test]
    fn executor_rejects_overlap_and_encodes_stop() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default());
        assert!(executor.start(prepare_transfer(plan, &mut descriptor, &mut payload).unwrap())
                        .is_ok());
        let (mut descriptor2, mut payload2) = mappings(plan);
        let failure = executor.start(prepare_transfer(plan,
                                                      &mut descriptor2,
                                                      &mut payload2).unwrap())
                              .unwrap_err();
        assert_eq!(failure.error, ExecutorError::Busy);
        cancel_prepared(&failure.prepared, &mut descriptor2, &mut payload2).unwrap();
        let completion = executor.stop().unwrap();
        finish_transfer(completion, &mut descriptor, &mut payload).unwrap();
        assert_eq!(executor.into_inner().writes.last().copied(),
                   Some((plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)));
    }

    #[test]
    fn mapping_mismatch_and_payload_sync_failure_are_recoverable() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let wrong = DmaRegion::new(0xa000, 0x4000, 512, 32, 64).unwrap();
        let mut wrong_payload = DmaMapping::new(wrong, DmaDirection::FromDevice,
                                               MockCache::default());
        assert!(matches!(prepare_transfer(plan, &mut descriptor, &mut wrong_payload),
                         Err(DriverError::InvalidParam)));
        assert!(descriptor.cpu_region().is_ok());

        let payload_region = payload.cpu_region().unwrap();
        payload = DmaMapping::new(payload_region,
                                  DmaDirection::FromDevice,
                                  MockCache { fail_device : true, ..MockCache::default() });
        assert!(matches!(prepare_transfer(plan, &mut descriptor, &mut payload),
                         Err(DriverError::IoError)));
        assert!(descriptor.cpu_region().is_ok());
        assert!(payload.cpu_region().is_ok());
    }

    #[test]
    fn typestate_session_covers_irq_completion() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default());

        let running = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                          .start(&mut executor)
                                                                          .unwrap();
        running.complete_irq().unwrap()
               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn prepared_session_is_returned_when_low_level_executor_is_busy() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let (mut descriptor2, mut payload2) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default());
        let prepared = prepare_transfer(plan, &mut descriptor, &mut payload).unwrap();
        executor.start(prepared).unwrap();

        let failure = match prepare_session(plan, &mut descriptor2, &mut payload2).unwrap()
                                                                                       .start(&mut executor) {
            Err(failure) => failure,
            Ok(_) => panic!("busy executor accepted a second session"),
        };
        assert_eq!(failure.error, ExecutorError::Busy);
        failure.session.cancel().unwrap();
        finish_transfer(executor.stop().unwrap(), &mut descriptor, &mut payload).unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert!(descriptor2.is_cpu_owned());
        assert!(payload2.is_cpu_owned());
    }

    #[test]
    fn quiesced_session_retries_only_mapping_still_owned_by_device() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let descriptor_region = DmaRegion::new(0x4000, 0x2000, 64, 32, 64).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x3000, 512, 32, 64).unwrap();
        let mut descriptor = DmaMapping::new(
            descriptor_region,
            DmaDirection::ToDevice,
            MockCache { fail_cpu_syncs : 1, ..MockCache::default() });
        let mut payload = DmaMapping::new(payload_region,
                                          DmaDirection::FromDevice,
                                          MockCache::default());
        let mut executor = Executor::new(MockOrderIo::default());
        let quiesced = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                           .start(&mut executor)
                                                                           .unwrap()
                                                                           .complete_irq().unwrap();
        let failure = quiesced.finish().unwrap_err();
        assert_eq!(failure.error, DriverError::IoError);
        failure.session.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn typestate_stop_quiesces_before_restoring_cpu_ownership() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default());
        prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                               .start(&mut executor).unwrap()
                                                               .stop().unwrap()
                                                               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().writes.last().copied(),
                   Some((plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)));
    }
}
