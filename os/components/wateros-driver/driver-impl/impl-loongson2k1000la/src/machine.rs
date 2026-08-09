use api_v0::{DriverResult, MachineDriver};

pub struct Machine;

static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver { &MACHINE }

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> { crate::init_after_boot() }

    fn realtime_ns(&self) -> DriverResult<Option<u64>> { crate::unsupported_realtime() }

    fn handle_external_interrupt(&self, snapshot : usize) -> DriverResult<()> {
        if snapshot == 0 || snapshot & !0xff != 0 {
            return Err(api_v0::DriverError::InvalidParam);
        }
        // UNVERIFIED_ON_HARDWARE: persistent topology/LIOINTC ownership is not
        // assembled yet. Refuse service rather than returning with a live,
        // unmasked level interrupt and causing an implicit re-enable.
        Err(api_v0::DriverError::Unsupported)
    }

    fn test(&self) { crate::self_test(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_snapshot_validation_fails_closed_until_runtime_exists() {
        assert_eq!(MACHINE.handle_external_interrupt(0),
                   Err(api_v0::DriverError::InvalidParam));
        assert_eq!(MACHINE.handle_external_interrupt(1 << 8),
                   Err(api_v0::DriverError::InvalidParam));
        assert_eq!(MACHINE.handle_external_interrupt(1 << 3),
                   Err(api_v0::DriverError::Unsupported));
    }
}
