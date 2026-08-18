# Machine Driver 公共 DTB 解析离线手册

[Driver 总览](../../README.md) · [Driver API v0](../../driver-api/api-v0/README.md)

本 crate 只放多个 machine profile 都复用、无硬件副作用的 DTB助手。VirtIO寄存器探测、
PCI BAR、DMA、IRQ注册和设备发布必须留在 RV/LA machine impl。

## 1. 入口与前置条件

```text
platform::dtb_pa()
  -> common::dtb::read_fdt(pa)
  -> fdt.all_nodes()
       -> compatible_list(node)
       -> first_mmio_region(node)
       -> parse_irq(node)
  -> machine 构造 DeviceInfo
  -> transport probe / registry
```

`read_fdt`：

- `pa==0` -> `NotFound`；
- `unsafe Fdt::from_ptr` 失败 -> `InvalidDtb`；
- 返回 `Fdt<'static>`，这个 `'static` 是由 raw pointer 人工承诺，不是复制了 DTB。

调用者必须保证 DTB从调用开始到内核结束都映射可读且不会被 frame allocator覆盖。RISC-V
MM会按 `total_size` 排除 DTB页；当前 LoongArch MM忽略 `_dtb_pa`，这是需要修复的跨模块
生命周期缺口。

`Fdt::from_ptr` 在验证 header 前就需要读取指针指向内存；任意用户指针或未映射 PA不能
传入。分页后还依赖恒等映射/DMW。

## 2. helper 精确语义

### 2.1 `read_be_u32(raw, offset)`

DTB cells 是大端，函数读取 `[offset,offset+4)` 并转 `u32`，不足四字节返回 `None`。
当前 `offset + 4` 没有 checked add；若未来 offset 来自不可信复杂计算，应先修成
`offset.checked_add(4)`，避免 debug 溢出 panic/release绕回。

函数不验证 4-byte 对齐，源码注释把对齐责任交给调用者。

### 2.2 `first_mmio_region(node)`

调用 `node.reg()`，只取第一项；缺属性、无法解析、缺 size或 size=0 返回 `None`。FDT库
负责根据父节点 address/size-cells形成 `starting_address/size`，本 helper 不做额外 bus
range translation验证。

多 region/BAR设备只保留首项，不能据此完成完整驱动。若设备要求 control+doorbell 两个
窗口，应新增返回 `Vec<MmioRegion>` 的 API或 transport 专属解析。

函数不检查 base+size 溢出、平台窗口、RAM重叠或页对齐；machine probe 必须检查。

### 2.3 `parse_irq(node)`

只读 `interrupts` 的第一个 big-endian u32，并直接读节点自身的 `interrupt-parent`首 cell。
缺任一必要内容时返回 `None`。

它不支持：

- `#interrupt-cells > 1` 中的 trigger/type等后续 cells；
- 从父节点继承 `interrupt-parent`；
- `interrupts-extended`；
- 多条 IRQ；
- MSI/MSI-X、GPIO级联和 controller-specific translation。

因此 `Some(IrqLine)` 只对仓库当前简单 DTB形态有效；复杂节点应返回 Unsupported并由
专属 parser处理，不能静默截第一 cell。

### 2.4 `compatible_list(node)`

读取 NUL 分隔 `compatible`，跳过空片段和非法 UTF-8，保留原顺序。完全非法会得到空
Vec，而不是错误。绑定使用精确字符串；不可做 substring/prefix匹配。

规范 DTB compatible 应为 ASCII，但静默丢弃非法片段可能隐藏损坏。安全/诊断加强时可
返回 `DriverResult<Vec<String>>` 并区分 missing 与 malformed。

### 2.5 `is_virtio_mmio_compatible`

只在列表中精确查找 `"virtio,mmio"`。命中只说明 transport 类型，不能推断块/网卡/GPU；
必须再读 VirtIO MMIO magic/version/device-id/vendor-id。

## 3. 新增 parser 实例：完整 IRQ specifier

推荐放在 machine/interrupt-controller 专属层：

1. 找显式或继承的 interrupt-parent phandle；
2. 定位 controller node并读取 `#interrupt-cells`；
3. 验证 `interrupts` 长度是 cells*4 的整数倍；
4. checked读取每组 cell；
5. 按 controller compatible解释 irq/type/polarity；
6. 返回能表达多 IRQ和触发方式的新类型；
7. 绑定 IRQ前验证 CPU context/PLIC route。

不要扩大当前 `IrqLine` 的含义却仍只保存一个 u32，这会让消费者无法发现信息丢失。

## 4. 新增 parser 实例：多 MMIO region

```rust
pub fn mmio_regions(node: FdtNode<'_, '_>) -> DriverResult<Vec<MmioRegion>> {
    let regs = node.reg().ok_or(DriverError::NotFound)?;
    let mut out = Vec::new();
    for region in regs {
        let size = region.size.ok_or(DriverError::InvalidDtb)?;
        if size == 0 || (region.starting_address as usize).checked_add(size).is_none() {
            return Err(DriverError::InvalidParam);
        }
        out.push(MmioRegion { base: region.starting_address as usize, size });
    }
    Ok(out)
}
```

真实实现还要使用 fallible allocation策略并验证 bus `ranges`。不要在 common parser中
直接 ioremap 或读寄存器，保持解析纯函数，方便合成 DTB单测。

## 5. 锁、生命周期和分配

FDT对象借用原始 blob，没有内部锁。DTB按设计只读，因此多个 CPU只读扫描可以并发；
但 machine init guard 通常只允许一次扫描。`compatible_list` 和多 region API会分配，必须
在 heap ready 后调用。

不要把 `FdtNode` 或借用的 property bytes 存入长期 registry；registry应保存拥有数据的
`DeviceInfo/String/Vec`。这能防止未来 DTB映射释放导致悬垂引用，但原始 Fdt扫描期间仍
要求 blob有效。

## 6. 故障定位

### 扫描零设备

检查 platform dtb_pa、FDT magic/total_size、物理映射、DTB是否被 allocator覆盖；再打印
node name和完整 compatibles。不要先怀疑子系统 registry。

### MMIO probe magic错误

确认 `reg` 是否多段且首段确为 control window、父 bus ranges是否已转换、size/对齐、
内核页表是否映射该物理窗。`first_mmio_region` 不证明这些条件。

### IRQ不触发

检查 `#interrupt-cells`。若大于 1，当前 parse_irq只保存第一 cell，很可能丢了类型/flags；
还要核对 interrupt-parent继承、controller初始化和路由。

## 7. 自回归

用最小合成 DTB覆盖：

- pa=0、坏 magic、截断 total_size；
- `#address-cells/#size-cells` 为 1/2，64-bit reg；
- 无 reg、零 size、多 reg、base+size溢出；
- compatible 单项/多项/尾 NUL/空项/非法 UTF-8；
- interrupts 1/2/3 cells、多个 IRQ、继承 parent、interrupts-extended；
- DTB位于 frame pool 中时的启动保护；
- RV和 LA machine 使用同一 helper后的设备快照。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
