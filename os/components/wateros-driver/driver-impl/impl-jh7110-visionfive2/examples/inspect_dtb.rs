use std::{env, fs};
use wateros_driver_impl_jh7110_visionfive2::topology::discover;

fn main() {
    let mut args = env::args().skip(1);
    let first = args.next().expect("usage: inspect_dtb [--expect-invalid] <file.dtb>");
    let (expect_invalid, path) = if first == "--expect-invalid" {
        (true, args.next().expect("missing invalid DTB path"))
    } else {
        (false, first)
    };
    let bytes = fs::read(path).expect("read DTB");
    let bytes = Box::leak(bytes.into_boxed_slice());
    let result = discover(bytes.as_ptr() as usize);
    if expect_invalid {
        assert!(result.is_err(), "malformed DTB was unexpectedly accepted");
        println!("malformed fixture rejected: {:?}", result.unwrap_err());
        return;
    }
    let topology = result.expect("discover topology");
    let uart = topology.console_uart
                       .expect("chosen UART");
    let plic = topology.plic
                       .as_ref()
                       .expect("PLIC");
    assert_eq!(uart.mmio.base, 0x1000_0000);
    assert_eq!(plic.mmio.base, 0x0C00_0000);
    assert_eq!(plic.sources, 136);
    assert_eq!(plic.contexts.len(), 4);
    assert_eq!(plic.contexts[0].hart_id, Some(0));
    assert_eq!(plic.contexts[1].hart_id, Some(0));
    assert_eq!(plic.contexts[2].hart_id, Some(1));
    assert_eq!(plic.contexts[3].hart_id, Some(1));
    assert_eq!(plic.context_for_hart(0), Some(1));
    assert_eq!(plic.context_for_hart(1), Some(3));
    assert_eq!(topology.mmc_hosts
                       .len(),
               2);
    assert_eq!(topology.mmc_hosts[0].mmio
                                    .base,
               0x1601_0000);
    assert_eq!(topology.mmc_hosts[0].irq, 74);
    assert_eq!(topology.mmc_hosts[0].bus_width, 8);
    assert!(topology.mmc_hosts[0].non_removable);
    assert_eq!(topology.mmc_hosts[0].biu_clock.args, vec![91]);
    assert_eq!(topology.mmc_hosts[0].ciu_clock.args, vec![92]);
    assert_eq!(topology.mmc_hosts[0].reset.args, vec![90]);
    let sysreg = topology.mmc_hosts[0].sysreg.expect("MMC0 sysreg");
    assert_eq!((sysreg.offset, sysreg.shift, sysreg.mask),
               (0x14, 0x1a, 0x7c00_0000));
    assert_eq!(topology.mmc_hosts[1].mmio
                                    .base,
               0x1602_0000);
    assert_eq!(topology.mmc_hosts[1].irq, 75);
    assert_eq!(topology.mmc_hosts[1].bus_width, 4);
    assert_eq!(topology.mmc_hosts[1].biu_clock.args, vec![93]);
    assert_eq!(topology.mmc_hosts[1].ciu_clock.args, vec![94]);
    assert_eq!(topology.mmc_hosts[1].reset.args, vec![95]);
    println!("fixture topology: {topology:?}");
}
