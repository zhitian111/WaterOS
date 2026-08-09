use std::{env, fs};

use wateros_driver_impl_loongson2k1000la::{irq_binding::resolve,
                                           irq_entry::resolve_parent_line,
                                           irq_runtime::RuntimeLayout,
                                           irq_plan::{ActivationPolicy, OwnerKind,
                                                      compile as compile_owner_plan},
                                           mmc::{ActivationBlocker, PrerequisiteStatus, plan},
                                           topology::{MmcClockProvider, SupplyProvider, discover}};

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next()
                   .expect("usage: verify_topology <valid|invalid> <dtb>");
    let bytes = fs::read(args.next()
                             .expect("missing DTB path")).expect("read DTB");
    let fdt = fdt::Fdt::new(&bytes).expect("parse DTB");
    match mode.as_str() {
        "valid" => {
            let topology = discover(&fdt).expect("discover valid topology");
            let runtime_layout = RuntimeLayout::compile(&topology)
                .expect("compile fixed IRQ runtime layout");
            assert_eq!(runtime_layout.controllers[0].main_base, 0x1FE0_1400);
            assert_eq!(runtime_layout.controllers[1].main_base, 0x1FE0_1440);
            assert_eq!(runtime_layout.parent_banks[2], Some(0));
            assert_eq!(runtime_layout.parent_banks[3], Some(1));
            let owner_plan = compile_owner_plan(&topology).expect("compile IRQ owner plan");
            assert_eq!(owner_plan.entries[0].kind, OwnerKind::MmcCommand);
            assert_eq!(owner_plan.entries[0].binding.global_irq().raw(), 31);
            assert_eq!(owner_plan.entries[0].hardware_line, 2);
            assert_eq!(owner_plan.entries[0].route.parent_line, 0);
            assert_eq!(owner_plan.entries[0].policy, ActivationPolicy::AckOnly);
            assert_eq!(owner_plan.entries[1].kind, OwnerKind::ApbDmaDeferred);
            assert_eq!(owner_plan.entries[1].binding.global_irq().raw(), 45);
            assert_eq!(owner_plan.entries[1].hardware_line, 3);
            assert_eq!(owner_plan.entries[1].route.parent_line, 1);
            assert_eq!(owner_plan.entries[1].policy, ActivationPolicy::Deferred);
            assert_eq!(topology.interrupt_controllers
                               .len(),
                       2);
            assert_eq!(topology.uarts.len(), 1);
            assert_eq!(topology.mmc_hosts
                               .len(),
                       1);
            assert_eq!(topology.dma_controllers.len(), 1);
            let dma = &topology.dma_controllers[0];
            assert_eq!(dma.mmio.base, 0x1FE0_0C10);
            assert_eq!(dma.mmio.size, 8);
            assert_eq!(&dma.interrupt.cells[..2], &[13, 4]);
            let dma_irq = resolve(&topology, &dma.interrupt)
                .expect("resolve APBDMA interrupt provider");
            assert_eq!(dma_irq.global_irq().bank(), 1);
            assert_eq!(dma_irq.global_irq().local(), 13);
            assert_eq!(dma.clock.specifier.args, &[12]);
            assert_eq!(dma.channel_cells, 1);
            let intc = &topology.interrupt_controllers[0];
            assert_eq!(intc.main_mmio.base, 0x1FE0_1400);
            assert_eq!(intc.core_isr.len(), 2);
            assert_eq!(intc.core_isr[0].base, 0x1FE0_1040);
            assert_eq!(intc.core_isr[1].base, 0x1FE0_1140);
            assert_eq!(intc.interrupt_cells, 2);
            assert_eq!(intc.parent_source_maps, [u32::MAX,
                                                 0,
                                                 0,
                                                 0]);
            assert_eq!(intc.parent_interrupts[0].as_ref()
                                                .expect("int0")
                                                .cells[0],
                       2);
            let intc1 = &topology.interrupt_controllers[1];
            assert_eq!(intc1.parent_source_maps, [0,
                                                  u32::MAX,
                                                  0,
                                                  0]);
            assert_eq!(intc1.parent_interrupts[1].as_ref()
                                                 .expect("int1")
                                                 .cells[0],
                       3);
            assert_eq!(resolve_parent_line(&topology, 2).expect("resolve HWI2").bank, 0);
            assert_eq!(resolve_parent_line(&topology, 3).expect("resolve HWI3").bank, 1);
            let uart = &topology.uarts[0];
            assert_eq!(uart.mmio.base, 0x1FE2_0000);
            assert_eq!(uart.clock_hz, 125_000_000);
            assert_eq!(&uart.interrupt.cells[..2], &[0, 4]);
            assert_eq!(uart.interrupt
                           .parent_phandle,
                       intc.phandle
                           .expect("referenced LIOINTC phandle"));
            let mmc = &topology.mmc_hosts[0];
            assert_eq!(mmc.controller_mmio
                          .base,
                       0x1FE2_C000);
            assert_eq!(mmc.auxiliary_mmio
                          .expect("second MMC region")
                          .base,
                       0x1FE0_0438);
            assert_eq!(&mmc.interrupt.cells[..2], &[31, 4]);
            assert_eq!(mmc.clocks.len(), 1);
            assert_eq!(mmc.clocks[0].specifier
                                    .args,
                       &[12]);
            assert_eq!(mmc.clock_provider,
                       MmcClockProvider::Loongson2k {
                           mmio : api_v0::MmioRegion { base : 0x1FE0_0480, size : 0x58 },
                           reference_hz : 100_000_000,
                       });
            assert_eq!(mmc.dma
                          .as_ref()
                          .expect("MMC DMA")
                          .name
                          .as_deref(),
                       Some("rx-tx"));
            assert_eq!(mmc.dma
                          .as_ref()
                          .expect("MMC DMA")
                          .specifier
                          .args,
                       &[0]);
            assert_eq!(mmc.bus_width, 4);
            match &mmc.card_detect {
                wateros_driver_impl_loongson2k1000la::topology::CardDetect::Gpio(line) => {
                    assert_eq!(line.specifier.args, &[22, 1]);
                    assert_eq!(line.pin, 22);
                    assert!(line.active_low);
                    assert!(matches!(line.provider,
                                     wateros_driver_impl_loongson2k1000la::topology::GpioProvider::Loongson2k1000 {
                                         mmio : api_v0::MmioRegion { base : 0x1FE0_0500,
                                                                     size : 0x38 },
                                         ngpios : 64,
                                     }));
                }
                other => panic!("unexpected card detect: {other:?}"),
            }
            assert!(mmc.vmmc_supply
                       .is_some());
            assert!(mmc.vqmmc_supply
                       .is_some());
            assert!(matches!(mmc.vmmc_supply.unwrap().provider,
                             SupplyProvider::Fixed { always_on : false,
                                                     boot_on : false,
                                                     gpio_controlled : false }));
            let plan = plan(mmc).expect("build deferred MMC plan");
            assert!(!plan.can_activate());
            assert!(plan.blockers
                        .contains(&ActivationBlocker::DataPathUnavailable));
            assert_eq!(plan.prerequisites.clock, PrerequisiteStatus::RequiresDriver);
            assert_eq!(plan.prerequisites.vmmc, PrerequisiteStatus::RequiresDriver);
            assert_eq!(plan.prerequisites.vqmmc, PrerequisiteStatus::RequiresDriver);
            assert_eq!(plan.prerequisites.card_detect, PrerequisiteStatus::RequiresDriver);
        }
        "invalid" => assert!(discover(&fdt).is_err()),
        "non-removable" => {
            let topology = discover(&fdt).expect("discover non-removable MMC");
            assert!(matches!(topology.mmc_hosts[0].card_detect,
                             wateros_driver_impl_loongson2k1000la::topology::CardDetect::NonRemovable));
        }
        _ => panic!("unknown mode: {mode}"),
    }
}
