# runtime-serial

此 crate 仅再导出字符设备层的 QEMU virt UART API，供运行期需要读写 serial device 的
代码使用。它不负责 early boot 输出，也不应绕过 `runtime-console` 用于内核日志。

early console 属于 `platform::console`；runtime serial 属于已经完成驱动注册后的字符
设备层。两者共享物理 UART 时必须通过各自既有锁路径访问，不能并行直接 MMIO。
