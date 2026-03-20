fn main() {
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/asm/_start.S");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");
}
