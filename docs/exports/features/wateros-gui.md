# wateros-gui — 已实现功能快照

## 定位

`wateros-gui` 是可选的内核窗口系统。它不参与默认比赛构建，根 feature `gui` 同时启用
GUI、`driver/display` 和 `driver/input`；旧 `display-demo` feature 转发到 `gui`。

## crate 结构

| crate | 职责 |
|-------|------|
| `wateros-gui` | 聚合 API 与当前软件实现 |
| `wateros-gui-api-v0` | 颜色、几何、文本、输入、窗口、控件和语义事件 |
| `wateros-gui-impl-software` | shadow surface、Canvas、字体、窗口合成、事件路由与全局 runtime |

## 已实现能力

- BGRA8888 shadow framebuffer 和显示设备短锁提交。
- 裁剪、alpha、矩形、线、圆、多边形、区域 blit。
- 可打印 ASCII 5×7 字体、整数缩放、测量、换行和水平/垂直对齐。
- 最多 16 个脏矩形；相交区域合并，容量溢出退化为安全的大区域。
- 多窗口 z 序、活动状态、拖动、关闭请求、指针命中和键盘焦点。
- `Panel`、`Label`、`Button`、`ProgressBar`、`TextInput`。
- 有界输入/输出队列和 `Clicked`、`TextChanged`、`Submitted` 等语义事件。
- VirtIO 键盘/平板原始事件转换、US 键盘布局、修饰键、相对/绝对指针。
- 可替换主题、指定显示设备初始化、shutdown/reinitialize 和运行快照。

## 双架构后端

| 架构 | 显示 | 输入 |
|------|------|------|
| RISC-V QEMU virt | VirtIO GPU MMIO | VirtIO keyboard/tablet MMIO |
| LoongArch QEMU virt | VirtIO GPU PCI | VirtIO keyboard/tablet PCI |

## 当前边界

- 当前只有内核窗口，不提供 `/dev/fb0`、用户态窗口协议或用户态 GUI 库。
- 字体为 ASCII，键盘为 US 布局，尚无 Unicode/IME。
- 输入使用有预算的周期轮询，尚未接设备中断。
- GUI 内部按脏区绘制和复制；VirtIO GPU 0.12 后端区域提交仍退化为全屏 flush。
- 单个 runtime 绑定一个显示器；API 可选择设备索引，尚未同时合成多屏。

## 验证

```bash
make check ARCH=rv PROFILE=pre EXTRA_FEATURES=gui
make check ARCH=la PROFILE=pre EXTRA_FEATURES=gui
cargo test --manifest-path components/wateros-gui/gui-impl/impl-software/Cargo.toml
python3 -m unittest scripts.tests.test_qemu_run
```

