# Input Device API v0 离线开发手册

[Driver 总览](../../../README.md) · [Driver API](../../../driver-api/api-v0/README.md)

本 crate 提供 VirtIO/Linux evdev风格原始事件、静态设备信息、非阻塞取事件接口和全局
registry。按键映射、鼠标加速、TTY行规程、GUI焦点和阻塞等待属于消费者。

## 1. 数据结构

- `InputDeviceKind::{Keyboard,Pointer,Unknown}` 是主要用途；组合设备可能仍需从事件能力
  判断，不能仅靠 kind；
- `AbsoluteAxis {minimum,maximum}` 是闭区间。构造时必须保证 min<=max；归一化要处理
  min==max和减法溢出；
- `InputDeviceInfo` 在设备生命周期内稳定，name拥有 String，absolute_x/y可选；
- `RawInputEvent {event_type,code,value}` 与 evdev三元组兼容，但没有 timestamp。

事件流仍包含 EV_SYN/SYN_REPORT分帧语义。消费者不能把每个 ABS_X和ABS_Y分别当完整
鼠标状态；应累积到 SYN_REPORT再发布一帧。value含按键 0/1/2、相对增量或绝对值，含义
由 type/code决定。

## 2. trait 与调用链

```text
machine probe -> VirtIO input feature/config
  -> InputDeviceInfo + event queue
  -> register_input_device
IRQ或轮询
  -> 驱动回收 used descriptors并放内部事件队列
consumer poll
  -> clone SharedInputDevice
  -> device spin mutex
  -> pop_event() -> Some/None/Err
  -> evdev/TTY/GUI转换
```

`pop_event` 必须非阻塞：无事件返回 `Ok(None)`，不能在持 spin mutex时睡眠。阻塞
`read(/dev/input/event*)` 应由 VFS waitqueue在锁外睡眠，IRQ入队后唤醒，再重试 pop。

trait只要求 `Send`；外层 `Arc<Mutex<Box<_>>>` 提供共享串行化。

## 3. 队列与丢包

API没有规定内部容量、overflow事件或 wake callback。后端必须使用有界预分配队列，不能
在 IRQ/spin锁内无限 Vec增长。满时应记录 drop并最好注入 SYN_DROPPED语义，让消费者
重新同步设备状态；静默丢一个 key-up会造成“按键永久按下”。

一次 `pop_event` 只取一个三元组。高吞吐路径可未来增加 batch pop，但要保持单事件
API兼容。

## 4. 注册表

registry只追加，index稳定，无 first helper、去重、注销或热拔插。 `input_devices()`
在锁内 clone全部 Arc并进行不可失败 Vec分配，不能在 IRQ或低内存关键路径调用；已知
index时优先 `input_device_at`。

设备 info返回引用，只能在设备 mutex guard内使用；需要锁外显示时 clone info。

## 5. 新后端/消费者实例

新增 PS/2键盘后端：

1. 构造稳定 name/kind，无 absolute axes；
2. IRQ中读取 scan code、转换成 evdev EV_KEY + SYN_REPORT；
3. 入有界 ring并唤醒外部 waitqueue；
4. `pop_event` 只做 dequeue，不睡眠/分配；
5. 全部 IRQ初始化成功后 register；
6. 定义溢出、未知 scan code和设备移除策略。

新增 syscall/ioctl时，原始 event ABI需显式序列化字段宽度/端序和时间戳；不能直接把
Rust struct布局 memcpy给用户。

## 6. 回归

- keyboard press/repeat/release + SYN；
- pointer REL_X/REL_Y/button，tablet ABS范围边界；
- 组合设备/Unknown和缺失 axis；
- 空队列立即 None，满队列丢包恢复；
- IRQ与消费者并发，不能在设备锁内唤醒后反向死锁；
- registry snapshot OOM策略和重复注册；
- 用户 read被 signal打断、nonblocking EAGAIN和设备拔出。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

