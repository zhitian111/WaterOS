# 一级组件关系总图

事实来源：`os/Cargo.toml`（根依赖）、`os/feature-tree.txt`（feature 展开）。下图描述 **根 crate 直接依赖** 与主线运行时数据流，非 workspace 全量 crate 列表。

## 根依赖总览

```mermaid
flowchart TB
    subgraph root["wateros（根）"]
        main["main.rs / trap_handler / bring-up"]
    end

  subgraph always["始终链接"]
    base["wateros-base<br/>+ base-config"]
    platform["wateros-platform<br/>arch + board impl"]
    runtime["wateros-runtime"]
    mm["wateros-mm"]
    task["wateros-task"]
    syscall["wateros-syscall"]
    abi["wateros-abi"]
    utils["wateros-utils"]
  end

  subgraph qemu["QEMU 主线 optional"]
    driver["wateros-driver"]
    fs["wateros-fs"]
    vfs["wateros-vfs"]
    ipc["wateros-ipc"]
    cred["wateros-cred"]
    klog["wateros-klog"]
  end

  main --> always
  main --> qemu

  syscall --> abi
  syscall --> task
  syscall --> mm
  syscall --> vfs
  syscall --> ipc
  syscall --> cred
  syscall --> klog
  syscall --> driver

  vfs --> fs
  vfs --> task
  vfs --> ipc

  fs --> driver

  cred --> task
  ipc --> task

  mm --> vfs
  driver --> fs

  platform --> runtime
  task --> platform
  mm --> platform
```

## 分层语义

```mermaid
flowchart LR
  subgraph L0["L0 地基"]
    base
    abi
    utils
  end

  subgraph L1["L1 机器抽象"]
    platform
    runtime
  end

  subgraph L2["L2 内核服务"]
    mm
    task
    ipc
    cred
    klog
  end

  subgraph L3["L3 I/O"]
    driver
    fs
    vfs
  end

  subgraph L4["L4 用户边界"]
    syscall
  end

  L0 --> L1 --> L2 --> L3 --> L4
  L2 --> L4
  L1 --> L4
```

| 层级 | 组件 | 职责 |
|------|------|------|
| L0 | base, abi, utils | 类型、常量、syscall ABI；utils 暂为空壳 |
| L1 | platform, runtime | trap/时间/串口、panic/堆/开发日志 |
| L2 | mm, task, ipc, cred, klog | 地址空间、调度、同步/信号、凭证、内核环 |
| L3 | driver, fs, vfs | 硬件、块 FS/伪 FS、路径与 fd 语义 |
| L4 | syscall | trap 入口、Linux 号表分发 |

## 平台 feature 分叉

两主线在根 `Cargo.toml` 中互斥选用，均打开完整 I/O 栈（driver + fs + ipc + cred + vfs-bridge + klog + syscall/impl-kernel）。

```mermaid
flowchart TB
  subgraph riscv["qemu-riscv64-opensbi（默认）"]
    r_plat["platform/impl-qemu-riscv64-opensbi"]
    r_arch["arch/impl-riscv64"]
    r_mm["mm/impl-sv39"]
    r_drv["driver/impl-qemu-riscv64-opensbi<br/>virtio-mmio"]
    r_ipc["ipc/all + impl-riscv64"]
    r_vfs["vfs + fd-session + impl-riscv64"]
    r_log["runtime/impl-warn + klog"]
  end

  subgraph la["qemu-loongarch64-virt"]
    l_plat["platform/impl-qemu-loongarch64-virt"]
    l_arch["arch/impl-loongarch64"]
    l_mm["mm/impl-loongarch64"]
    l_drv["driver/impl-qemu-loongarch64-virt<br/>virtio-pci"]
    l_ipc["ipc/all + impl-loongarch64"]
    l_vfs["vfs + fd-session + impl-loongarch64"]
    l_log["runtime/impl-error + klog"]
  end

  abi64["abi/impl-linux-generic64"] --> riscv
  abi64 --> la
```

共性：`task/impl-core`、`fs` 默认 ext4-rs + devfs、`syscall/impl-kernel`、`driver/impl-block-cache`。

## 组件内 api / impl 模式

每个一级组件（除 base、utils）普遍采用：

```text
<component>/
  *-api/api-v0/     # 稳定 trait 与类型
  *-impl/impl-*/    # 可替换实现
  src/lib.rs        # active_impl + 再导出
```

`platform` 额外拆分 `platform-arch`（ISA）与 `platform-impl`（板级）；`driver` 再分 block / character / network 子系统。详见 [`module-relations.md`](module-relations.md)。

## 未纳入根依赖

- `wateros-pseudo-shell`：feature `pseudo-shell` 可选。
- `arch_api_v0`：根直接依赖平台 arch API，供 `trap_handler` 等使用。
- workspace 内大量 `*-api-v0`、`*-impl-*` 子 crate 由组件聚合层 feature 拉入，不单独出现在根 `Cargo.toml`。
