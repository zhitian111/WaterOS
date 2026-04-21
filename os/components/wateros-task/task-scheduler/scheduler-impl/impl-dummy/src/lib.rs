#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use base::sync::UniprocessorSafeCell;
use core::arch::global_asm;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use riscv::register::sstatus;
use task_api::{KernelTask, KernelTaskEntry, TaskContext, TaskId, TaskStatus, IDLE_TASK_ID};

global_asm!(include_str!("switch.S"));

unsafe extern "C" {
    fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
    fn __task_entry();
}

const KERNEL_TASK_STACK_SIZE: usize = 32 * 1024;

#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);

struct ScheduledTask {
    public: KernelTask,
    _stack: Box<AlignedKernelStack>,
    is_idle: bool,
}

impl ScheduledTask {
    fn new(id: TaskId, entry: KernelTaskEntry, arg: usize, is_idle: bool) -> Self {
        let stack = Box::new(AlignedKernelStack([0; KERNEL_TASK_STACK_SIZE]));
        let stack_bottom = stack.0.as_ptr() as usize;
        let kernel_stack_top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE, 16);
        let mut task_cx = TaskContext::goto_entry(__task_entry as usize, kernel_stack_top);
        task_cx.s[0] = entry as usize;
        task_cx.s[1] = arg;
        Self {
            public: KernelTask {
                id,
                status: TaskStatus::Ready,
                task_cx,
                kernel_stack_top,
                entry,
            },
            _stack: stack,
            is_idle,
        }
    }
}

struct RoundRobinScheduler {
    bootstrap_task_cx: TaskContext,
    current: Option<Box<ScheduledTask>>,
    ready_queue: VecDeque<Box<ScheduledTask>>,
    idle_task: Option<Box<ScheduledTask>>,
    next_task_id: TaskId,
}

impl RoundRobinScheduler {
    fn new() -> Self {
        Self {
            bootstrap_task_cx: TaskContext::zero_init(),
            current: None,
            ready_queue: VecDeque::new(),
            idle_task: None,
            next_task_id: IDLE_TASK_ID + 1,
        }
    }

    fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.current = None;
        self.ready_queue.clear();
        self.idle_task = Some(Box::new(ScheduledTask::new(
            IDLE_TASK_ID,
            idle_task_main,
            0,
            true,
        )));
        self.next_task_id = IDLE_TASK_ID + 1;
    }

    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.ready_queue
            .push_back(Box::new(ScheduledTask::new(task_id, entry, arg, false)));
        task_id
    }

    fn prepare_first_switch(&mut self) -> (*mut TaskContext, *const TaskContext) {
        let mut next = if let Some(task) = self.ready_queue.pop_front() {
            task
        } else {
            self.idle_task
                .take()
                .expect("idle task must exist before first run")
        };
        next.public.status = TaskStatus::Running;
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_cx_ptr = &next.public.task_cx as *const TaskContext;
        self.current = Some(next);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    fn rotate_current_and_pick_next(&mut self) -> Option<(*mut TaskContext, *const TaskContext)> {
        let mut current = self.current.take()?;
        let current_id = current.public.id;
        let current_ptr = &mut current.public.task_cx as *mut TaskContext;

        if current.is_idle {
            let Some(mut next) = self.ready_queue.pop_front() else {
                current.public.status = TaskStatus::Running;
                self.current = Some(current);
                return None;
            };
            next.public.status = TaskStatus::Running;
            let next_ptr = &next.public.task_cx as *const TaskContext;
            self.idle_task = Some(current);
            self.current = Some(next);
            return Some((current_ptr, next_ptr));
        }

        current.public.status = TaskStatus::Ready;
        self.ready_queue.push_back(current);

        let mut next = if let Some(task) = self.ready_queue.pop_front() {
            task
        } else {
            self.idle_task
                .take()
                .expect("idle task must exist before scheduling")
        };

        if next.public.id == current_id {
            next.public.status = TaskStatus::Running;
            self.current = Some(next);
            return None;
        }

        next.public.status = TaskStatus::Running;
        let next_ptr = &next.public.task_cx as *const TaskContext;
        self.current = Some(next);
        Some((current_ptr, next_ptr))
    }

    fn current_task_id(&self) -> Option<TaskId> { self.current.as_ref().map(|task| task.public.id) }
}

static mut SCHEDULER: MaybeUninit<UniprocessorSafeCell<RoundRobinScheduler>> = MaybeUninit::uninit();
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

fn scheduler_cell() -> &'static UniprocessorSafeCell<RoundRobinScheduler> {
    assert!(
        SCHEDULER_READY.load(Ordering::Acquire),
        "scheduler not initialized: call init_scheduler() first"
    );
    unsafe { &*SCHEDULER.as_ptr() }
}

fn with_scheduler<R>(f: impl FnOnce(&mut RoundRobinScheduler) -> R) -> R {
    let mut scheduler = scheduler_cell().exclusive_access();
    f(&mut scheduler)
}

struct InterruptGuard {
    restore_sie: bool,
}

impl InterruptGuard {
    fn new() -> Self {
        let restore_sie = sstatus::read().sie();
        unsafe {
            sstatus::clear_sie();
        }
        Self { restore_sie }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.restore_sie {
            unsafe {
                sstatus::set_sie();
            }
        }
    }
}

#[inline]
const fn align_down(value: usize, align: usize) -> usize { value & !(align - 1) }

extern "C" fn idle_task_main(_arg: usize) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_entry(entry_addr: usize, arg: usize) -> ! {
    let entry: KernelTaskEntry = unsafe { core::mem::transmute(entry_addr) };
    unsafe {
        sstatus::set_sie();
    }
    entry(arg)
}

pub fn init_scheduler() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        unsafe {
            SCHEDULER.write(UniprocessorSafeCell::new(RoundRobinScheduler::new()));
        }
        SCHEDULER_READY.store(true, Ordering::Release);
    }
    with_scheduler(|scheduler| scheduler.init());
    log::info!("[task-scheduler] initialized");
}

pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.spawn_kernel_task(entry, arg))
}

pub fn run_first_task() -> ! {
    let (current_task_cx_ptr, next_task_cx_ptr) =
        with_scheduler(|scheduler| scheduler.prepare_first_switch());
    unsafe {
        __switch(current_task_cx_ptr, next_task_cx_ptr);
    }
    panic!("run_first_task must not return");
}

pub fn suspend_current_and_run_next() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.rotate_current_and_pick_next());
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn schedule_tick() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.rotate_current_and_pick_next());
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn current_task_id() -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_id())
}
