# Linux MMC 供电拓扑参考

本页记录 2K1000 MMC 供电建模所依据的 Linux 主线上游语义，避免把测试夹具误当成真实板级事实。

## 一手来源与结论

- `Documentation/devicetree/bindings/regulator/fixed-regulator.yaml`：固定稳压器的 GPIO 是可选控制；普通 `regulator-fixed` 只要求 compatible 与 regulator-name，固定电压以相等的 min/max 表示。
- `drivers/regulator/fixed.c`：普通无 GPIO 固定稳压器使用空的 regulator operations；GPIO 通过 optional descriptor 获取。驱动明确同时覆盖可控与不可控固定供电。
- `drivers/mmc/core/regulator.c`：MMC 的 `vmmc` 与 `vqmmc` 都按 optional regulator 获取，缺少 supply 不是拓扑错误。
- `arch/loongarch/boot/dts/loongson-2k1000-ref.dts`：主线参考板 MMC 节点没有声明 `vmmc-supply` 或 `vqmmc-supply`。

来源：

- <https://www.kernel.org/doc/Documentation/devicetree/bindings/regulator/fixed-regulator.yaml>
- <https://github.com/torvalds/linux/blob/master/drivers/regulator/fixed.c>
- <https://github.com/torvalds/linux/blob/master/drivers/mmc/core/regulator.c>
- <https://github.com/torvalds/linux/blob/master/arch/loongarch/boot/dts/loongson-2k1000-ref.dts>

## WaterOS 边界

- 未声明 supply 表示由板级隐式供电，不等同于“缺失且不可用”。
- 显式无 GPIO 的 fixed regulator 没有软件 enable 动作，因此拓扑层可判定无需电源控制驱动。
- 显式 GPIO-controlled fixed regulator 仍需要 WaterOS GPIO 写路径；`regulator-always-on` 和 `regulator-boot-on` 只保留为策略证据，不能替代对 GPIO 状态的初始化或观察。
- 上述判断仅说明软件所有权与启动前置条件，不证明物理电压存在。真实供电、电压稳定性及上电时序均为 `UNVERIFIED_ON_HARDWARE`。
- 仓库的完整 fixture 保留合成 fixed-regulator 节点以覆盖解析器；测试另生成省略两路 supply 的 upstream-shaped 变体。
