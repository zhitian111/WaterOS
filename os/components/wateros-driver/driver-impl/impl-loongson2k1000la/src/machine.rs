use api_v0::{DriverResult, MachineDriver};

pub struct Machine;

static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver { &MACHINE }

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> { crate::init_after_boot() }

    fn realtime_ns(&self) -> DriverResult<Option<u64>> { crate::unsupported_realtime() }

    fn test(&self) { crate::self_test(); }
}
