//! 平台 console 组合层实现。

pub use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
    use base::sync::MultiprocessorSafeCell;
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// 写控制台时临时屏蔽本核中断，并在离开时恢复原始状态。
    ///
    /// IPI_SYNC: 这只避免同一 CPU 的中断重入；跨 CPU 串行化由
    /// [`CONSOLE_WRITE_LOCK`] 完成。不要在持有 scheduler 锁时调用控制台。
    struct ConsoleInterruptGuard(Option<arch::interrupt::ArchInterruptState>);

    impl ConsoleInterruptGuard {
        #[inline]
        fn new() -> Self {
            let state = arch::interrupt::read_global_interrupt_state().ok();
            let _ = arch::interrupt::disable_global_interrupt();
            Self(state)
        }
    }

    impl Drop for ConsoleInterruptGuard {
        fn drop(&mut self) {
            if let Some(state) = self.0 {
                let _ = arch::interrupt::restore_global_interrupt_state(state);
            }
        }
    }

    /// 运行期可选的控制台接收端。OS 在完整 UART 字符设备注册后安装它；
    /// 在此之前必须保持 `None`，以便 early console 仍能用于引导日志。
    pub type RuntimeConsoleWriter = fn(&[u8]) -> PlatformConsoleResult<()>;
    /// driver 层注册后的运行期 writer；`None` 时回退到 early-console profile。
    static RUNTIME_CONSOLE_WRITER: MultiprocessorSafeCell<Option<RuntimeConsoleWriter>> =
        MultiprocessorSafeCell::new(None);
    /// 串行化跨 CPU 的整段输出，覆盖 runtime UART 尚未注册的 early boot 阶段。
    static CONSOLE_WRITE_LOCK: MultiprocessorSafeCell<()> = MultiprocessorSafeCell::new(());
    const NO_CONSOLE_OWNER: usize = usize::MAX;
    /// 当前持锁 CPU，用于识别同 CPU 的嵌套日志并避免递归获取自旋锁。
    static CONSOLE_WRITE_OWNER: AtomicUsize = AtomicUsize::new(NO_CONSOLE_OWNER);

    /// 安装运行期控制台写入端。
    ///
    /// BOOT_CONTRACT: 只能在字符设备和其内部锁已完全初始化后调用；替换 writer 时
    /// 必须由启动序列保证没有并发输出，当前接口不提供注销或热替换同步。
    pub fn register_runtime_writer(writer: RuntimeConsoleWriter) {
        *RUNTIME_CONSOLE_WRITER.exclusive_access() = Some(writer);
    }

    /// 写一个字节；换行转换和后端错误语义由选中的 console profile 决定。
    #[inline]
    fn runtime_writer() -> Option<RuntimeConsoleWriter> {
        *RUNTIME_CONSOLE_WRITER.exclusive_access()
    }

    /// 在中断屏蔽和跨 CPU 输出锁保护下执行一次完整写入。
    ///
    /// `write(true)` 表示当前 CPU 已递归持锁：此时只能走 early profile，不能再次
    /// 获取 runtime writer 的锁，避免格式化日志重入导致死锁。
    fn with_console_write_lock<R>(write: impl FnOnce(bool) -> R) -> R {
        let _interrupt_guard = ConsoleInterruptGuard::new();
        let cpu = arch::cpu::current_cpu_id().raw();
        let guard = match CONSOLE_WRITE_LOCK.try_lock() {
            Some(guard) => guard,
            None if CONSOLE_WRITE_OWNER.load(Ordering::Acquire) == cpu => {
                return write(true);
            }
            None => CONSOLE_WRITE_LOCK.exclusive_access(),
        };

        CONSOLE_WRITE_OWNER.store(cpu, Ordering::Release);
        let result = write(false);
        CONSOLE_WRITE_OWNER.store(NO_CONSOLE_OWNER, Ordering::Release);
        drop(guard);
        result
    }

    /// 原子性边界为整个 `bytes` 缓冲，而不是单个字符，避免多核日志行互相穿插。
    #[inline]
    pub fn console_write_a_byte(byte: u8) -> PlatformConsoleResult<()> {
        console_write_a_buffer(core::slice::from_ref(&byte))
    }

    /// 请求后端将已经写入的字节送出；不隐含全局内存屏障或设备驱动 drain。
    #[inline]
    pub fn console_write_a_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
        with_console_write_lock(|reentrant| {
            if !reentrant {
                if let Some(writer) = runtime_writer() {
                    return writer(bytes);
                }
            }
            crate::active_impl::console::console_write_a_buffer(bytes)
        })
    }

    /// 在内核日志使用的同一跨 CPU 锁下写入精确终端线缆字节；板级后端不做 CR/LF 转换。
    pub fn console_write_raw_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
        with_console_write_lock(|reentrant| {
            if !reentrant {
                if let Some(writer) = runtime_writer() {
                    return writer(bytes);
                }
            }
            crate::active_impl::console::console_write_raw_buffer(bytes)
        })
    }

    /// 在同一次底层控制台锁持有期间完成完整格式化操作。
    ///
    /// `fmt::Write` 可以多次调用 `write_str`，因此不能把锁放在 `Writer` 内的每次
    /// 回调中，否则一条格式化日志仍可能被别的 CPU 插入。
    pub fn console_write_fmt(args: core::fmt::Arguments<'_>) -> PlatformConsoleResult<()> {
        struct Writer(Option<RuntimeConsoleWriter>);
        impl core::fmt::Write for Writer {
            fn write_str(&mut self, value: &str) -> core::fmt::Result {
                if let Some(writer) = self.0 {
                    writer(value.as_bytes()).map_err(|_| core::fmt::Error)
                } else {
                    crate::active_impl::console::console_write_a_buffer(value.as_bytes())
                        .map_err(|_| core::fmt::Error)
                }
            }
        }

        with_console_write_lock(|reentrant| {
            let writer = if reentrant { None } else { runtime_writer() };
            core::fmt::Write::write_fmt(&mut Writer(writer), args)
                .map_err(|_| PlatformConsoleError::WriteFailure)
        })
    }

    #[inline]
    pub fn console_flush() -> PlatformConsoleResult<()> {
        crate::active_impl::console::console_flush()
    }
