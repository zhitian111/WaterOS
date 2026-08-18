# `/dev/null` 字符设备实现手册

本 crate 实现最小的 null 字符设备。上层契约见
[character-api](../../character-api/api-v0/README.md)，devfs 注册关系见
[driver-character](../../README.md)。

## 1. 精确语义

`NullCharacterDevice` 是无状态零大小类型：

- `read(buf)` 永远返回 `Ok(0)`，表示 EOF，不修改缓冲；
- `write(buf)` 返回 `Ok(buf.len())`，所有字节立即丢弃；
- `device_kind()` 返回 `CharacterDeviceKind::Null`；
- ioctl 沿用 trait 默认值 `DriverError::Unsupported`；
- poll 沿用默认实现：请求 `POLLIN`/`POLLOUT` 时都报告就绪。

`POLLIN` 就绪与 read 返回 EOF 并不矛盾：EOF 是可立即完成的读状态。空写返回 0；大写入只
返回长度，不分配与复制第二份缓冲。

它不是 `/dev/zero`：后者读取必须填零而非 EOF；也不是 sink 日志后端，因为没有计数和诊断。

## 2. 注册与访问链

```text
driver bring-up
  -> register_builtin_character_devices
  -> register_null_stub
     -> Box<NullCharacterDevice>
     -> Arc<Mutex<Box<dyn CharacterDevice>>>
     -> register_character_device -> index
  -> devfs manager 扫描 kind=Null
  -> 建立 /dev/null alias

read/write/poll
  -> fd-session/devfs handle
  -> character registry index
  -> 设备 mutex
  -> CharacterDevice 方法
```

全局注册表只保存设备对象，文件打开/offset/flags/close 生命周期属于 VFS。重复注册会产生多个
Null index，devfs alias 选择可能变化，所以 builtin 注册函数应在一次 boot 中只调用一次。

## 3. 锁和 user-copy 边界

设备本身无可变状态，却仍包装在统一 mutex 中。syscall/VFS 必须先完成用户地址验证和分块
copy，再调用 write；不能因为数据会被丢弃就跳过用户地址可读性检查，否则无效用户指针会
错误成功。read 返回 EOF 时无需写用户缓冲，因此长度非零也不应触发 copy-to-user。

设备 mutex 内没有阻塞操作。若未来加入统计计数，应使用 checked/saturating 或原子计数，
不要让观测功能改变 null 的成功语义。

## 4. 新增 `/dev/zero` 的正确方式

不要给 `NullCharacterDevice` 加模式分支。应新增 `CharacterDeviceKind::Zero` 和独立实现：

1. `read` 把内核缓冲填 0 并返回完整长度；
2. `write` 同样丢弃并返回长度；
3. poll 永远可读写；
4. devfs 建立唯一 `/dev/zero` alias；
5. VFS 仍在锁外对用户页逐段 copy，处理跨页 EFAULT 与部分进度；
6. 测试 mmap 若要支持，应在 VFS/MM 层定义匿名零页语义，不能靠字符 read 假装完成。

## 5. 故障与回归

- `cat /dev/null` 不退出：read 被错误映射成 WouldBlock，而非 EOF；
- 写无效指针仍成功：上层跳过了 user-copy 校验；
- `/dev/null` 不存在或指错：检查 builtin 是否注册、kind 是否为 Null、devfs alias 扫描顺序；
- poll 忙循环本身正常，因为 EOF/readable 永久成立，调用程序应在 read=0 后停止。

回归包括 read 长度 0/非 0、write 长度 0/大缓冲、无效用户指针、poll masks、重复 open/close、
dup/fork 与并发访问。此实现无需硬件运行测试，但要随 RV/LA 全量 check 编译。

