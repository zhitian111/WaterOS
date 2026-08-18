//! `wateros` 内核 crate 的构建脚本：按启用的 board feature 把平台链接脚本与
//! 入口 shim 登记为 `rerun-if-changed`，并向链接器传入 `-T.../link.ld`。
//!
//! 平台 impl 用 `global_asm!` 编入 `_start.S`，入口符号必须与
//! `link.ld` 中的 `ENTRY(...)` 一致；架构 impl 则负责通用 boot 汇编。

/// 按启用的 board feature 向 Cargo 声明重链条件并传入链接脚本。
///
/// 构建脚本只能通过标准输出中的 `cargo::` 指令影响当前编译：
/// `rerun-if-changed` 保证链接脚本或入口汇编变化时重新执行，
/// `rustc-link-arg` 则把板级链接脚本传给最终链接步骤。两个架构分支
/// 由 feature 条件编译，未选中的平台不会把另一架构的入口符号或内存
/// 布局带入链接；若同时启用互斥平台，最终链接可能出现重复入口或布局
/// 冲突，应由顶层 feature 配置在更早阶段拒绝。
fn main() {
    // 公共段布局始终参与依赖追踪；它与板级脚本共同决定内核各段的地址。
    println!("cargo::rerun-if-changed=./components/wateros-platform/linker/kernel-sections.ld");

    // RISC-V 的 `_start.S` 必须和对应 link.ld 一起重新链接，否则入口符号
    // 或栈/段地址变化不会反映到最终内核镜像中。
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/asm/_start.S");
    #[cfg(feature = "qemu-riscv64-opensbi")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-riscv64-opensbi/src/linker/link.ld");

    // LoongArch 使用独立的入口汇编和链接布局；保持与 RISC-V 分支隔离，
    // 避免把不兼容的 ISA 指令或入口符号传给错误的目标架构。
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rerun-if-changed=./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/asm/_start.S");
    #[cfg(feature = "qemu-loongarch64-virt")]
    println!("cargo::rustc-link-arg=-T./components/wateros-platform/platform-impl/\
              impl-qemu-loongarch64-virt/src/linker/link.ld");
}
