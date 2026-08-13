#![no_std]
//! WaterOS 终端行规程的版本化公开数据契约。

/// 与 Linux 兼容的终端控制字符数量。
pub const NCCS: usize = 19;

pub const ICRNL: u32 = 0x100;
pub const OPOST: u32 = 0x1;
pub const ONLCR: u32 = 0x4;
pub const ISIG: u32 = 0x1;
pub const ICANON: u32 = 0x2;
pub const ECHO: u32 = 0x8;
pub const TOSTOP: u32 = 0x100;

pub const VINTR: usize = 0;
pub const VQUIT: usize = 1;
pub const VERASE: usize = 2;
pub const VKILL: usize = 3;
pub const VEOF: usize = 4;
pub const VTIME: usize = 5;
pub const VMIN: usize = 6;
pub const VSUSP: usize = 10;

pub const SIGINT: usize = 2;
pub const SIGQUIT: usize = 3;
pub const SIGHUP: usize = 1;
pub const SIGCONT: usize = 18;
pub const SIGTSTP: usize = 20;
pub const SIGWINCH: usize = 28;

/// WaterOS 内核中稳定标识一个终端实例的编号。
///
/// `1` 固定表示系统控制台；伪终端使用独立编号。该编号只用于内核模块间路由，
/// 不是 Linux 设备号，也不会作为用户 ABI 暴露。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TerminalId(u64);

impl TerminalId {
    /// 系统串口控制台的固定终端编号。
    pub const CONSOLE: Self = Self(1);

    /// 从内核注册表分配的原始值构造终端编号。
    pub const fn from_raw(raw: u64) -> Self { Self(raw) }

    pub const fn raw(self) -> u64 { self.0 }
}

/// 一个终端文件描述符连接到的端点类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalEndpoint {
    Console,
    PtyMaster,
    PtySlave,
}

/// 创建或打开伪终端时可直接映射到 VFS errno 的错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyError {
    NotFound,
    Locked,
    NoSpace,
    Invalid,
    HungUp,
    WouldBlock,
    Interrupted,
    Busy,
}

/// 系统控制台标准输入的数据来源策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleTtyMode {
    /// 消费来自物理平台控制台的输入字节。
    Interactive,
    /// 读取立即返回 EOF，供无人值守评测使用。
    Closed,
    /// 提供 pre/LTP 兼容测试需要的固定密码输入。
    Fixture,
}

/// 行规程使用的架构无关 `termios` 状态。
///
/// Linux ABI 转换保留在 syscall 层，因此该类型不包含 syscall 请求号或用户指针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TtyTermios {
    pub iflag: u32,
    pub oflag: u32,
    pub cflag: u32,
    pub lflag: u32,
    pub line: u8,
    pub cc: [u8; NCCS],
}

impl TtyTermios {
    /// 适用于串口登录控制台的类 Linux 初始配置。
    pub const DEFAULT: Self = Self {
        iflag: 0x500,
        oflag: 0x5,
        cflag: 0xbf,
        lflag: 0x8a3b,
        line: 0,
        cc: [3, 28, 127, 21, 4, 0, 1, 0, 17, 19, 26, 0, 18, 15, 23, 22, 0, 0, 0],
    };
}

/// 通过 `TIOCGWINSZ` 返回的终端窗口尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TtyWinSize {
    pub row: u16,
    pub col: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

impl TtyWinSize {
    pub const DEFAULT: Self = Self { row: 25, col: 80, xpixel: 0, ypixel: 0 };
}

/// 处理终端控制字符后产生的信号请求。
///
/// 为避免锁顺序反转，实际投递必须在释放 TTY 锁后由 syscall/signal 层完成。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtyControlEvent {
    pub process_group: usize,
    pub signal: usize,
}
