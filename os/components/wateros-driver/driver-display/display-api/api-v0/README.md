# Display Device API v0 离线开发手册

[Driver 总览](../../../README.md) · [Driver API](../../../driver-api/api-v0/README.md)

本 crate 描述单 scanout、线性 BGRA framebuffer、刷新操作和显示设备注册表。Canvas、
字体、GUI、VirtIO transport及用户 mmap在其他层。

## 1. 结构与不变量

`FramebufferInfo`：

```text
width/height       像素尺寸
stride             相邻行首字节距离
format             当前仅 Bgra8888（B,G,R,A各8 bit）
byte_len           可绘制有效缓冲长度
phys_base          DMA物理起点，仅设备 mmap层可暴露
mapped_len         DMA页对齐映射长度，可能大于 byte_len
base               当前内核可访问地址，仅诊断
```

构造器必须 checked验证：

```text
row_bytes = width * 4
stride >= row_bytes
needed = stride * height
byte_len >= needed
mapped_len >= byte_len
phys_base/mapped_len 满足页映射和设备DMA限制
framebuffer().len() >= byte_len
```

API结构本身不验证这些不变量。错误 stride/len会使绘制层越界或把 padding当像素。普通
绘制代码只能使用 `framebuffer()`，不能直接解引用 phys_base/base。

`FramebufferRegion` 是左上角 `(x,y)` 与非零 width/height。合法性需用 checked add
验证 `x+width<=screen.width`、`y+height<=screen.height`；API默认 `flush_region`
不会验证，只退化为全屏 flush。

## 2. 锁和调用链

```text
machine GPU probe -> 建 DMA framebuffer/scanout
  -> register_display_device(Arc<Mutex<Box<dyn DisplayDevice>>>)
GUI/Canvas
  -> clone device Arc
  -> device.lock()
  -> info
  -> framebuffer() 借用 &mut [u8]
  -> 绘制
  -> flush_region/full flush
  -> 释放 device lock
```

framebuffer借用不能超过 mutex guard。Rust借用通常会保证这一点，但不要通过 raw pointer
缓存跨 unlock；GPU重置/模式切换可能替换 backing。

当前锁覆盖整次 CPU绘制和 VirtIO提交，大面积软件渲染会阻塞其他刷新/mmap查询。优化可
用双缓冲或单独 framebuffer锁，但必须定义提交中的 buffer所有权。

## 3. trait 语义

- `info(&self)` 返回当前固定模式；API没有 mode-change通知；
- `framebuffer(&mut self)` 成功返回可写完整 slice；
- `flush` 必须把软件内容 transfer到 host resource并提交 scanout；
- `flush_region` 不支持时安全全刷；支持时只传合法矩形；
- flush成功只表示设备接受/完成当前同步命令，不保证人眼已显示。

错误不得留下 VirtIO queue descriptor永久占用。区域 flush对 stride/pixel offset的计算
必须 checked，不能假设 `stride==width*4`。

## 4. 注册和 mmap生命周期

registry只追加，无注销/去重，index按枚举顺序稳定。getter clone Arc后释放 registry锁。
注册分配不可失败，重复 init会重复GPU。

用户 framebuffer mmap必须持有独立 `DeviceMappingLease`/Arc，保证 fd关闭或 registry
变化时 DMA页仍存在；unmap只删 PTE，不能交给普通 frame allocator。mmap长度使用
`mapped_len`，但用户可访问/报告的有效像素仍是 `byte_len`。

## 5. 新增后端实例

新增 simplefb：

1. 从 DTB验证 base/size/stride/format；
2. 建立 MMIO/write-combining映射并构造完整 info；
3. framebuffer返回映射 slice；
4. 若无需设备提交，flush可做必要 memory barrier/cache clean后成功；
5. 不支持区域更新时保留默认全刷；
6. 完整 ready后注册并创建 VFS framebuffer描述；
7. 明确 framebuffer内存由固件、驱动还是 frame allocator拥有。

## 6. 当前边界与回归

当前无多 scanout、光标 plane、EDID/mode set、vsync、damage queue、热拔插和多 pixel
format。没有 API级 `FramebufferInfo::validate`，每个后端可能重复检查。

回归：尺寸乘法溢出、stride padding、mapped_len页尾、零/越界 region、全屏与局部刷新、
连续1000次 flush descriptor回收、GPU reset、两个绘制者锁序、用户 mmap在 fd/进程退出
后的 lease，以及无GPU时 registry为空。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

