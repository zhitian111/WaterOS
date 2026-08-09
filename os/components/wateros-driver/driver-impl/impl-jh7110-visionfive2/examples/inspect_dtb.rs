use std::{env, fs};
use wateros_driver_impl_jh7110_visionfive2::topology::discover;

fn main() {
    let path = env::args().nth(1)
                          .expect("usage: inspect_dtb <file.dtb>");
    let bytes = fs::read(path).expect("read DTB");
    let bytes = Box::leak(bytes.into_boxed_slice());
    let topology = discover(bytes.as_ptr() as usize).expect("discover topology");
    let uart = topology.console_uart
                       .expect("chosen UART");
    let plic = topology.plic
                       .as_ref()
                       .expect("PLIC");
    assert_eq!(uart.mmio.base, 0x1000_0000);
    assert_eq!(plic.mmio.base, 0x0C00_0000);
    assert_eq!(plic.sources, 136);
    assert_eq!(plic.contexts.len(), 2);
    println!("fixture topology: {topology:?}");
}
