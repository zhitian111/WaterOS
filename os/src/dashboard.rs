//! 终端顶部的 CPU 状态面板。
//!
//! 此模块刻意不在 timer trap 中输出。串口是慢速设备，在中断上下文格式化并
//! 写出整张表会拉长中断延迟，也会让多个输出源更容易交错。面板由唯一的普通
//! 内核任务刷新；每次先生成完整 ANSI 帧，再通过 UART 字符设备的一次 `write`
//! 写出，因而与用户态 stdout 共用同一个设备锁。

extern crate alloc;

use alloc::string::String;
use core::fmt::{self, Display, Write};
use core::sync::atomic::{AtomicBool, Ordering};
use utils::table_format::{Alignment, Cell, Column, FixedTable, Overflow};

/// 每 tick 为 10ms；500ms 刷新一次既足够观察调度状态，也不会长期占用 UART。
const REFRESH_INTERVAL_TICKS : u64 = 50;
const MAX_CPUS : usize = base_config::task::MAX_CPUS;
/// 标题、空行、三条分隔线和 CPU 行组成的总行数。
const PANEL_HEIGHT : usize = MAX_CPUS + 6;
/// 普通 shell / 日志从这一行开始滚动，面板固定在它之前。
const FIRST_SCROLLABLE_ROW : usize = PANEL_HEIGHT + 1;
const CPU_COLUMNS : [Column; 7] =
    [Column::new(4, Alignment::Right).overflow(Overflow::Truncate(">")),
     Column::new(14, Alignment::Right).overflow(Overflow::Truncate(">")),
     Column::new(6, Alignment::Left).overflow(Overflow::Truncate(">")),
     Column::new(9, Alignment::Right).overflow(Overflow::Truncate(">")),
     Column::new(4, Alignment::Left).overflow(Overflow::Truncate(">")),
     Column::new(6, Alignment::Right).overflow(Overflow::Truncate(">")),
     Column::new(8, Alignment::Right).overflow(Overflow::Truncate(">"))];

static INITIALIZED : AtomicBool = AtomicBool::new(false);
static STARTED : AtomicBool = AtomicBool::new(false);

/// 初始化 dashboard 状态。
///
/// 此函数可在早期启动阶段调用，但不会触碰串口；真正的绘制要等字符设备注册
/// 完成并进入调度器之后，由 [`start`] 创建的后台任务完成。
pub fn init() { INITIALIZED.store(true, Ordering::Release); }

/// 启动唯一的后台刷新任务。应在 UART 字符设备注册完成后调用。
pub fn start() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    task::spawn_kernel_task(dashboard_task, 0);
}

extern "C" fn dashboard_task(_arg : usize) -> ! {
    let mut first_frame = true;
    loop {
        let frame = render_frame(first_frame);
        // `CharacterDevice` #0 是 QEMU 的主 UART。一次 write 在该设备的 Mutex
        // 保护下完成，用户态 echo/cat 的 stdout 会等待该帧结束，不能插入表格。
        let _ = driver::character::with_character_device(0, |device| {
            let _ = device.write(frame.as_bytes());
        });
        first_frame = false;
        task::sleep_for_ticks(REFRESH_INTERVAL_TICKS);
    }
}

/// 把一帧完整面板编码到单个 buffer。
///
/// 第一帧清屏并设置滚动区；之后保存当前 shell 光标、在顶部重绘、再恢复光标。
/// 普通输出始终留在 `FIRST_SCROLLABLE_ROW..`，不会把顶部面板滚走。
fn render_frame(first_frame : bool) -> String {
    let tick = task::current_tick();
    let states = task::cpu_states();
    let mut frame = String::with_capacity(1_600);

    if first_frame {
        // 先取消旧滚动区/原点模式，再清屏。面板总宽 70 列，可放入 QEMU 常见的
        // 80x24 终端，避免自动换行破坏后续光标定位。
        frame.push_str("\x1b[r\x1b[?6l\x1b[2J\x1b[H");
    } else {
        // DECSC / DECRC：QEMU 的 stdio 终端和常见 ANSI 终端均支持。
        frame.push_str("\x1b7\x1b[?6l\x1b[H");
    }

    let _ = writeln!(frame,
                     "+--------------- WaterOS CPU Dashboard (tick={}) ------------------+",
                     tick);
    let _ = writeln!(frame,
                     "|                                                                         |");
    if let Ok(mut table) = FixedTable::new(&CPU_COLUMNS).begin(&mut frame) {
        let _ = table.row(&[Cell::text("CPU"),
                            Cell::text("Current Task"),
                            Cell::text("State"),
                            Cell::text("Q O/F/R"),
                            Cell::text("Rsch"),
                            Cell::text("Switch"),
                            Cell::text("Timer")]);
        let _ = table.separator();

        for raw in 0..MAX_CPUS {
            let cpu_id = task::CpuId::from_raw(raw);
            let snapshot = states.iter()
                                 .find_map(|(id, state)| (*id == cpu_id).then_some(state));
            match snapshot {
                Some(state) => {
                    let current = TaskIdText(&state.current_task_id);
                    let queues = QueueCounts(state);
                    let _ = table.row(&[Cell::display(&raw),
                                        Cell::display(&current),
                                        Cell::text(cpu_state_label(state)),
                                        Cell::display(&queues),
                                        Cell::text(if state.need_resched { "YES" } else { "-" }),
                                        Cell::display(&state.context_switches),
                                        Cell::display(&state.timer_ticks)]);
                }
                None => {
                    let _ = table.row(&[Cell::display(&raw),
                                        Cell::text("-"),
                                        Cell::text("-"),
                                        Cell::text("-"),
                                        Cell::text("-"),
                                        Cell::text("-"),
                                        Cell::text("-")]);
                }
            }
        }
        let _ = table.finish();
    }
    frame.push('\n');

    if first_frame {
        // DECSTBM：仅第 15 行至屏幕底部可滚动。999 会由 ANSI 终端裁剪为实际
        // 最后一行，比省略底边界在部分终端上更可靠。
        let _ = write!(frame,
                       "\x1b[{};999r\x1b[{};1H",
                       FIRST_SCROLLABLE_ROW, FIRST_SCROLLABLE_ROW);
    } else {
        frame.push_str("\x1b8");
    }
    frame
}

/// 不分配地显示当前任务 ID。
struct TaskIdText<'a>(&'a Option<task::TaskId>);

impl Display for TaskIdText<'_> {
    fn fmt(&self, formatter : &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(id) => write!(formatter, "{}", id),
            None => formatter.write_str("-"),
        }
    }
}

/// 不分配地显示 Other/FIFO/RR 三类可运行队列长度。
struct QueueCounts<'a>(&'a task::CpuSnapshot);

impl Display for QueueCounts<'_> {
    fn fmt(&self, formatter : &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter,
               "{}/{}/{}",
               self.0
                   .runnable_other,
               self.0.runnable_fifo,
               self.0.runnable_rr)
    }
}

/// 一眼区分离线、尚未首次切换、idle、用户任务与内核任务。
fn cpu_state_label(state : &task::CpuSnapshot) -> &'static str {
    if !state.online {
        "OFF"
    } else if state.current_task_id
                   .is_none()
    {
        "BOOT"
    } else if state.current_is_idle {
        "IDLE"
    } else if state.current_is_user {
        "USER"
    } else {
        "KERN"
    }
}
