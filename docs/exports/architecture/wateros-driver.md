# wateros-driver — 组件关系

## 用途

描述 `wateros-driver` 内部子 crate 与根 `wateros` 的依赖关系（当前快照）。

## 总览

```mermaid
flowchart TB
    subgraph root [wateros]
        main[os/src/main.rs]
    end

    subgraph agg [wateros-driver]
        lib[src/lib.rs]
        api[driver-api-v0]
        blk[driver-block]
        chr[driver-character]
        net[driver-network]
        plat[driver-impl]
    end

    subgraph blk_impl [block impl]
        vmmio[impl-virtio-mmio]
        vpci[impl-virtio-pci]
        cache[impl-block-cache]
    end

    subgraph net_impl [network impl]
        nmmio[impl-virtio-mmio]
        npci[impl-virtio-pci]
        smoltcp[impl-smoltcp]
    end

    subgraph plat_impl [platform impl]
        rv[impl-qemu-riscv64-opensbi]
        la[impl-qemu-loongarch64-virt]
        dummy[impl-dummy]
    end

    main --> lib
    lib --> api
    lib --> blk
    lib --> chr
    lib --> net
    lib --> plat

    blk --> api
    blk --> vmmio
    blk --> vpci
    blk --> cache

    net --> api
    net --> nmmio
    net --> npci
    net --> smoltcp

    chr --> api

    plat --> rv
    plat --> la
    plat --> dummy

    rv --> blk
    rv --> net
    rv --> chr
    la --> blk
    la --> net
    la --> chr

    smoltcp --> vfs_api[wateros-vfs-api-v0]
    rv --> fs_agg[wateros-fs devfs]
    la --> fs_agg
```

## Feature 与平台路径

```mermaid
flowchart LR
    subgraph riscv [qemu-riscv64-opensbi]
        f1[impl-qemu-riscv64-opensbi]
        f2[block impl-virtio-mmio]
        f3[network impl-virtio-mmio]
        f4[impl-block-cache]
    end

    subgraph loong [qemu-loongarch64-virt]
        g1[impl-qemu-loongarch64-virt]
        g2[block impl-virtio-pci]
        g3[network impl-virtio-pci]
        g4[impl-block-cache]
    end

    f1 --> f2
    f1 --> f3
    f1 --> f4
    g1 --> g2
    g1 --> g3
    g1 --> g4
```

## 运行时数据流（块设备 → 文件系统）

```mermaid
sequenceDiagram
    participant boot as init_when_boot
    participant plat as active_impl
    participant blk as register_block_device
    participant cache as CachingBlockDevice
    participant devfs as fs devfs refresh
    participant ext4 as fs init ext4

    boot->>plat: save DTB PA
    plat->>plat: scan DTB or PCI
    plat->>cache: wrap VirtioBlk optional
    plat->>blk: register shared device
    plat->>devfs: refresh nodes
    ext4->>blk: read_blocks via devfs path
```

## 外部依赖

| 依赖组件 | 用途 |
|----------|------|
| `wateros-base-config` | RAM 回退、`BLOCK_CACHE_CAPACITY_BLOCKS` |
| `wateros-mm` 帧分配器 | VirtIO DMA（`frame_alloc_result`） |
| `wateros-fs` | devfs 刷新（平台 impl） |
| `wateros-vfs` | socket fd `VfsIoHandle`（`impl-smoltcp`） |
| `fdt` | RISC-V DTB 解析 |
| `virtio-drivers` | VirtIO MMIO/PCI transport |
| `smoltcp` | 内核协议栈 |

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 |
