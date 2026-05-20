//! 任务子系统最小自检：启动固定用户 ELF，并运行 pipe IPC 内核自检任务。
//!
//! **入口**：[`spawn_all`] 由 `kernel_main` 在 `fs::init` 之后调用。用户 ELF
//! 只负责从根卷装载并入队，真实用户态执行会在 `task::run_first_task()` 后发生。
//! pipe 自检任务覆盖阻塞读写、非阻塞错误、EOF 与 BrokenPipe。

use alloc::boxed::Box;
use runtime::logging::*;

/// 固定启动的用户态 hello world ELF。
const HELLO_WORLD_ELF_PATH : &str = "/elf/000_hello_world.elf";
const PIPE_SMOKE_ELF_PATH : &str = "/elf/010_pipe_smoke.elf";

/// 从根卷装载并启动 hello world 用户任务；若当前路径无根卷或 ELF，记录 warning 后跳过。
fn spawn_user_elf_task(path : &str, label : &str) {
    #[cfg(not(feature = "impl-sv39"))]
    {
        warn!("[task-selftest] impl-sv39 off: skip {}", path);
    }

    #[cfg(feature = "impl-sv39")]
    {
        match mm::kernel_mm::from_elf_path(path) {
            Ok(loaded) => {
                info!(
                    "[task-selftest] loaded {} path={} entry={:#x} image=[{:#x},+{:#x}) \
                     stack=[{:#x},{:#x}) satp={:#x} aspace_ptr={:#x}",
                    label,
                    path,
                    loaded.entry_pc,
                    loaded.image_base,
                    loaded.image_size,
                    loaded.stack_bottom,
                    loaded.stack_top,
                    loaded.satp,
                    loaded.user_aspace_ptr
                );
                let tid = task::spawn_user_task_from_loaded_elf(&loaded);
                #[cfg(feature = "vfs")]
                vfs::cwd::on_user_task_spawned(tid);
                info!("[task-selftest] spawned {} user task {}", label, tid);
            }
            Err(err) => {
                warn!(
                    "[task-selftest] skip {} path={}: {:?}",
                    label,
                    path,
                    err
                );
            }
        }
    }
}

fn pipe_from_arg(pipe_ptr : usize) -> &'static ipc::pipe::Pipe {
    unsafe { &*(pipe_ptr as *const ipc::pipe::Pipe) }
}

/// 读者先阻塞在空 pipe，写者稍后写入后应唤醒读者。
extern "C" fn pipe_reader_task(pipe_ptr : usize) -> ! {
    let pipe = pipe_from_arg(pipe_ptr);
    let mut buf = [0u8; 5];
    let n = pipe
        .read(&mut buf)
        .expect("pipe reader should be woken by writer");
    assert_eq!(n, buf.len(),
               "pipe reader must receive the full smoke payload");
    assert_eq!(&buf, b"water",
               "pipe reader payload must match writer payload");
    info!("[ipc-pipe] reader received {:?}", buf);
    task::exit_current(88);
}

/// 延迟写入，验证空 pipe reader 的条件等待不会丢唤醒。
extern "C" fn pipe_writer_task(pipe_ptr : usize) -> ! {
    let pipe = pipe_from_arg(pipe_ptr);
    task::sleep_for_ticks(2);
    let n = pipe
        .write(b"water")
        .expect("pipe writer should write while read end is open");
    assert_eq!(n, 5,
               "pipe writer must report the full smoke payload");
    info!("[ipc-pipe] writer sent {} bytes", n);
    task::exit_current(89);
}

/// pipe 已满时写者应阻塞，直到 reader 读出空间。
extern "C" fn pipe_full_writer_task(pipe_ptr : usize) -> ! {
    let pipe = pipe_from_arg(pipe_ptr);
    let n = pipe
        .write(&[9])
        .expect("full pipe writer should resume after reader frees space");
    assert_eq!(n, 1,
               "full pipe writer must write one byte after wake");
    info!("[ipc-pipe] full writer resumed");
    task::exit_current(90);
}

/// 稍后读出一个字节，释放满 pipe 的写者。
extern "C" fn pipe_full_reader_task(pipe_ptr : usize) -> ! {
    let pipe = pipe_from_arg(pipe_ptr);
    task::sleep_for_ticks(3);
    let mut buf = [0u8; 1];
    let n = pipe
        .read(&mut buf)
        .expect("full pipe reader should read prefilled byte");
    assert_eq!(n, 1,
               "full pipe reader must read one byte");
    assert_eq!(buf[0], 1,
               "full pipe reader must observe FIFO order");
    info!("[ipc-pipe] full reader freed one byte");
    task::exit_current(91);
}

/// 关闭写端后，空 pipe 读取应返回 EOF。
extern "C" fn pipe_eof_task(_arg : usize) -> ! {
    let pipe = ipc::pipe::Pipe::with_capacity(2).expect("pipe capacity should be valid");
    pipe.close_write();
    let mut buf = [0u8; 1];
    let n = pipe
        .read(&mut buf)
        .expect("closed write end should produce EOF");
    assert_eq!(n, 0,
               "empty pipe with closed writer must return EOF");
    info!("[ipc-pipe] eof check ok");
    task::exit_current(92);
}

/// 关闭读端后，写入应返回 BrokenPipe。
extern "C" fn pipe_broken_task(_arg : usize) -> ! {
    let pipe = ipc::pipe::Pipe::with_capacity(2).expect("pipe capacity should be valid");
    pipe.close_read();
    let err = pipe
        .write(&[1])
        .expect_err("closed read end should reject writes");
    assert_eq!(err,
               ipc::pipe::PipeError::BrokenPipe,
               "closed read end must map to BrokenPipe");
    info!("[ipc-pipe] broken pipe check ok");
    task::exit_current(93);
}

/// 非阻塞读空/写满路径应返回 WouldBlock。
extern "C" fn pipe_try_task(_arg : usize) -> ! {
    let pipe = ipc::pipe::Pipe::with_capacity(2).expect("pipe capacity should be valid");
    let mut buf = [0u8; 2];
    let read_err = pipe
        .try_read(&mut buf)
        .expect_err("empty open pipe should not be readable without blocking");
    assert_eq!(read_err,
               ipc::pipe::PipeError::WouldBlock,
               "empty open pipe try_read must return WouldBlock");
    assert_eq!(pipe
                   .try_write(&[7, 8])
                   .expect("try_write should fill empty pipe"),
               2,
               "try_write should fill two-byte pipe");
    let write_err = pipe
        .try_write(&[9])
        .expect_err("full pipe should not be writable without blocking");
    assert_eq!(write_err,
               ipc::pipe::PipeError::WouldBlock,
               "full pipe try_write must return WouldBlock");
    assert_eq!(pipe
                   .read(&mut buf)
                   .expect("pipe should read back buffered bytes"),
               2,
               "pipe should read back both buffered bytes");
    assert_eq!(&buf, &[7, 8],
               "pipe must preserve FIFO order in try path");
    info!("[ipc-pipe] try path check ok");
    task::exit_current(94);
}

/// 启动 hello world 用户任务与 pipe 内核自检任务。
pub fn spawn_all() {
    spawn_user_elf_task(HELLO_WORLD_ELF_PATH, "hello-world");
    spawn_user_elf_task(PIPE_SMOKE_ELF_PATH, "pipe-smoke");

    let pipe_smoke = Box::into_raw(Box::new(ipc::pipe::Pipe::with_capacity(8)
        .expect("pipe smoke capacity should be valid"))) as usize;
    let pipe_full = ipc::pipe::Pipe::with_capacity(4).expect("pipe full capacity should be valid");
    assert_eq!(pipe_full
                   .try_write(&[1, 2, 3, 4])
                   .expect("prefill pipe should succeed"),
               4,
               "prefill pipe should occupy full capacity");
    let pipe_full = Box::into_raw(Box::new(pipe_full)) as usize;

    let pipe_reader_task_id = task::spawn_kernel_task(pipe_reader_task, pipe_smoke);
    let pipe_writer_task_id = task::spawn_kernel_task(pipe_writer_task, pipe_smoke);
    let pipe_full_writer_task_id = task::spawn_kernel_task(pipe_full_writer_task, pipe_full);
    let pipe_full_reader_task_id = task::spawn_kernel_task(pipe_full_reader_task, pipe_full);
    let pipe_eof_task_id = task::spawn_kernel_task(pipe_eof_task, 0);
    let pipe_broken_task_id = task::spawn_kernel_task(pipe_broken_task, 0);
    let pipe_try_task_id = task::spawn_kernel_task(pipe_try_task, 0);

    info!("[ipc-pipe] spawned self-test tasks: reader={}, writer={}, full_writer={}, \
           full_reader={}, eof={}, broken={}, try={}",
          pipe_reader_task_id,
          pipe_writer_task_id,
          pipe_full_writer_task_id,
          pipe_full_reader_task_id,
          pipe_eof_task_id,
          pipe_broken_task_id,
          pipe_try_task_id);
}
