use std::{env, fs};

use wateros_platform_impl_loongson2k1000la::memory::primary_ram_from_fdt;

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next()
                   .expect("usage: verify_memory_dtb <official|missing> <dtb>");
    let path = args.next()
                   .expect("missing DTB path");
    assert!(args.next()
                .is_none(),
            "unexpected extra argument");
    let bytes = fs::read(path).expect("read DTB fixture");
    let fdt = fdt::Fdt::new(&bytes).expect("parse DTB fixture");
    let selected = primary_ram_from_fdt(&fdt);
    match mode.as_str() {
        "official" => {
            let selected = selected.expect("official layout must contain kernel RAM");
            assert_eq!(selected.start, 0x9000_0000);
            assert_eq!(selected.end, 0x2_7000_0000);
        }
        "missing" => assert!(selected.is_none()),
        _ => panic!("unknown verification mode: {mode}"),
    }
}
