# 调试、定位与回归

## 构建配置先固定

每份复现记录至少写明：

```text
ARCH=rv|la
PROFILE=pre|final
MODE=auto|shell|run
SMP=<1..8>
SDCARD=<image>
SNAPSHOT=0|1
EXTRA_FEATURES=<...>
kernel git diff/status
完整 guest 命令和首次异常日志
```

先执行：

```bash
make show-config ARCH=rv PROFILE=final
make check ARCH=rv PROFILE=final
```

`PROFILE` 决定 workload/镜像环境，`ARCH` 决定 MM、trap、platform 和 driver 实现。只写“QEMU 下失败”
不足以复现。

## 验证层级

| 层级 | 命令/方式 | 能证明什么 | 不能证明什么 |
| --- | --- | --- | --- |
| crate check | `cargo check --manifest-path ...` | 类型、feature 下的编译关系 | 顶层 feature 组合、运行行为 |
| 内核 check | `make check ARCH=... PROFILE=...` | 目标架构完整依赖可编译 | 启动、资源释放、并发 |
| build | `make build ...` | 链接脚本和最终内核可生成 | guest ABI 行为 |
| shell 定向测试 | `make shell ...` | 单路径运行结果和现场 | 自动队列完整性 |
| MODE=run | 指定 guest 脚本 | 可重复的小型集成测试 | 其它模块未回归 |
| auto workload | `make run ...` | 当前 profile 队列 | 未被队列覆盖的边界 |
| 压力/重复 | stress-ng/LTP，多轮资源比较 | 并发和生命周期趋势 | 另一架构行为 |

## 常用启动方式

```bash
make shell ARCH=rv PROFILE=final

make run ARCH=rv PROFILE=final \
  MODE=run SCRIPT=/glibc/my_test.sh

make run ARCH=la PROFILE=pre SMP=4
```

普通运行默认使用 snapshot。验证持久化时使用镜像副本并显式 `WRITE_DISK=1`，不要直接改基线镜像。

## QEMU 端口冲突

默认 user networking 将宿主 `127.0.0.1:2222` 转发到 guest 22。报错
`Could not set up host forwarding rule` 与内核编译 warning 无关。

```bash
lsof -nP -iTCP:2222 -sTCP:LISTEN

WOS_QEMU_HOSTFWD='tcp:127.0.0.1:2223-:22' \
make run ARCH=rv PROFILE=final

WOS_QEMU_HOSTFWD='' make run ARCH=rv PROFILE=final
```

确认 PID 后再终止旧实例；不要用宽泛的 `killall qemu`，它可能终止同机其它测试。

## panic/OOM 定位

看到 heap OOM 时记录 `layout_size`、`align`、`used/free/cap`。判断顺序：

1. 该分配是否由用户可控长度直接触发；
2. 容器是否使用不可失败的 `vec!`/`resize`；
3. 分配的是逻辑容量还是实际已用数据；
4. fork/dup 是否深复制本应共享的对象；
5. exit/reap 是否遗漏 registry、frame、fd 或 waiter；
6. 连续两轮测试后内存是否回到同一稳定值。

扩大 QEMU RAM 只能帮助判断阈值，不能证明泄漏已解决。WaterOS 的内核堆容量和 guest `MemTotal`
不是同一资源池。

## 内存/VMA 回归方法

在同一个新启动 guest 中记录：

```bash
cat /proc/meminfo
stress-ng --mmap 1 --mmap-file --mmap-bytes 16M --timeout 5s
cat /proc/meminfo
stress-ng --mmap 1 --mmap-file --mmap-bytes 16M --timeout 5s
cat /proc/meminfo
```

首轮允许出现一次性动态链接/缓存成本；第二轮不应再按映射大小持续下降。若下降，依次审计：

- VMA 是否把普通帧误标为外部所有；
- fork 是否增加共享帧引用；
- munmap/destroy 是否减少引用并在零时回收；
- lazy VMA、resident PTE 和共享文件 VMA 三份元数据是否同时删除；
- TLB flush 是否发生在 PTE 修改之后。

共享文件映射退出时，应区分 VFS `writeback()`（提交脏页）与 `flush()`（持久化/文件系统同步）。
`munmap/exit` 不是隐式 `fsync`。

## fork/pipe 压力回归

```bash
cat /proc/meminfo
stress-ng --forkheavy 4 --timeout 60s --metrics-brief
cat /proc/meminfo
```

检查四类证据：

- `successful run` 且 stressor failed 为 0；
- 没有 heap OOM/panic；
- 没有大量 exit/FD/VMA 清理 warning；
- 重复运行后的内存稳定。

`forkheavy` 会尝试扩大大量 pipe。pipe capacity 是流控上限，不意味着创建时必须立刻分配同等字节；
写入路径必须使用可失败分配并把失败转换为可恢复错误。

## 卡死和高 CPU

先区分：

- **宿主 QEMU 高 CPU、guest 无输出**：可能是用户态忙循环、重复 page fault/trap 或 benchmark 正常计算。
- **QEMU 低 CPU、guest 无输出**：可能是所有任务睡眠、lost wakeup、锁等待或定时器未重武装。
- **单个 CPU 100%**：检查同一 PC、同一 syscall/trap 是否重复。

使用 `make debug` 或 `make debug-server` + `make gdb`，采集所有 CPU PC、当前 task、trap cause 和锁记录。
页故障若返回同一用户 PC 而没有安装映射或发送信号，会形成 trap 风暴。

## 文件系统错误分层

| 失败位置 | 典型含义 | 应修层 |
| --- | --- | --- |
| syscall 参数校验 | flag/指针/fd/长度非法 | syscall |
| VFS route/handle | path、挂载、fd 类型、打开模式 | VFS |
| page cache writeback | 脏页状态或后端写区间 | VFS/FS bridge |
| FS operation | inode、目录项、空间、格式 | FS backend |
| block flush/I/O | VirtIO/设备能力或传输失败 | driver |

日志中只看到最终 `EIO` 时，在每层边界临时记录原始 error、path/identity、offset/len 和 open mode。
找到层次后保留低频失败日志，删除高频成功日志。

## LTP/benchmark 结果判读

- 以用例内部 `TPASS/TFAIL/TBROK/TCONF` 为准，不要盲信包装脚本中名称含 `FAIL` 的固定打印。
- 区分测试失败、脚本退出码、内核 warning、panic 四类事件。
- benchmark 的第一次运行可能包含缓存预热；展示数据要注明架构、profile、SMP、内存和镜像。
- 性能变慢但仍推进时，先看子项输出和宿主 CPU；静默不自动等于死锁。
- iozone 的 fsync `EIO` 与普通 write throughput 应分开记录，不能把未持久化结果写成完整通过。

## 提交前自回归清单

```bash
git diff --check
make check ARCH=rv PROFILE=pre
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

然后按改动范围选择运行测试：

- syscall ABI：最小 raw syscall + 对应 LTP case；
- MM/task/IPC：重复压力和资源基线；
- VFS/FS：读写、rename/unlink、fsync、fork 后句柄；
- platform/driver：双架构启动、IRQ/设备 I/O；
- 跨组件生命周期：完整 profile 自动队列。

最终记录必须包含没有完成的回归。被手工终止的测试不能写“通过”，只能记录已覆盖到的阶段。

