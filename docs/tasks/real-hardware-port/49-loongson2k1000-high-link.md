# 49 Loongson 2K1000 按高 cached 地址链接内核

## 任务内容

低链接 + 运行时加高基址的方案在 boot stack / data 访问上反复踩坑。
改为更本质的方案：

1. `link.ld` 的 `KERNEL_ENTRY_ADDRESS` 设为 `0x9000000098000000`
2. `kernel_global.rs` 从 `kernel_start` 推导窗口基址，按高 cached VA
   建立内核 RAM 页表，PPN 取低 48 位物理页号
3. MMIO 仍按低地址恒等映射

uImage legacy header 的 load/entry 仍写 32 位 `0x98000000`，由 U-Boot
`map_to_sysmem` 映射到高 cached 段。

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/linker/link.ld`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_global.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出进入 `WRS[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 高地址链接与高窗口页表映射完成；等待板端串口。
