//! 阻塞式伪 shell：经 MMIO UART 读行，调用 VFS 组合接口与（RISC-V 下）用户 ELF
//! 装载做 bring-up 验证。
//!
//! 本 crate 独立于 `wateros-runtime`，避免与 `mm-impl-sv39` → `runtime`
//! 的依赖形成环。
//!
//! **调用约定**：须在 `driver::active_impl::init_after_boot`（已初始化
//! UART）、`task::init` 与调度器 已运行之后，通常从 **内核任务** 入口调用
//! [`run_pseudo_shell`]；否则 `exec` 无法调度子任务。

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use runtime_serial::SerialPort;
use vfs::{
    mount, resolve_against_cwd, root, RootRwSession, SingleRootReadView, VfsDirEntry, VfsError,
    VfsFsKind, VfsNodeType, VfsResult,
};

const MAX_LINE : usize = 512;

/// 进入阻塞式 REPL；**不返回**。UART 经 [`runtime_serial::with_default_uart`]
/// 访问。
pub fn run_pseudo_shell() -> ! {
    let mut cwd = String::from("/");
    loop {
        let _ = runtime_serial::with_default_uart(|uart| {
            let _ = uart.write_all(b"wateros> ");
        });
        let mut line = [0u8; MAX_LINE];
        let n = read_line_into(&mut line);
        if n == 0 {
            continue;
        }
        let Ok(cmdline) = core::str::from_utf8(&line[..n]) else {
            let _ = runtime_serial::with_default_uart(|uart| {
                let _ = uart.write_all(b"error: invalid utf-8\n");
            });
            continue;
        };
        let cmdline = cmdline.trim();
        if cmdline.is_empty() {
            continue;
        }
        let mut parts = cmdline.split_whitespace();
        let cmd = parts.next()
                       .unwrap_or("");
        let arg = parts.next();
        let rest : Vec<&str> = parts.collect();

        let res : Result<(), VfsError> = match cmd {
            "help" | "?" => {
                let _ = runtime_serial::with_default_uart(|uart| {
                    let _ = uart.write_all(
                        b"commands: cd ls stat rm exec help\npaths: absolute or relative to cwd\n",
                    );
                });
                Ok(())
            }
            "cd" => do_cd(&mut cwd, arg),
            "ls" => do_ls(&cwd, arg),
            "stat" => do_stat(&cwd, arg),
            "rm" => do_rm(&cwd, arg),
            "exec" => do_exec(&cwd, arg, rest.as_slice()),
            _ => {
                let _ = runtime_serial::with_default_uart(|uart| {
                    let _ = uart.write_all(b"error: unknown command (try help)\n");
                });
                Ok(())
            }
        };

        if let Err(e) = res {
            let msg = format!("error: {:?}\n", e);
            let _ = runtime_serial::with_default_uart(|uart| {
                let _ = uart.write_all(msg.as_bytes());
            });
        }
    }
}

fn read_line_into(buf : &mut [u8]) -> usize {
    runtime_serial::with_default_uart(|uart| {
        let mut i = 0usize;
        loop {
            let b = uart.read_byte_blocking();
            if b == b'\r' {
                continue;
            }
            if b == b'\n' ||
               i >=
               buf.len()
                  .saturating_sub(1)
            {
                break;
            }
            buf[i] = b;
            i += 1;
        }
        i
    }).unwrap_or(0)
}

fn do_cd(cwd : &mut String, arg : Option<&str>) -> Result<(), VfsError> {
    let path = match arg {
        None | Some("") | Some("/") => String::from("/"),
        Some(a) => resolve_against_cwd(cwd.as_str(), Some(a))?,
    };
    let view = root::read_view();
    if !view.exists(path.as_str())? {
        return Err(VfsError::NotFound);
    }
    let m = view.metadata(path.as_str())?;
    if m.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotAFile);
    }
    *cwd = path;
    Ok(())
}

fn do_ls(cwd : &str, arg : Option<&str>) -> Result<(), VfsError> {
    let path = match arg {
        None | Some("") => cwd.to_string(),
        Some(a) => resolve_against_cwd(cwd, Some(a))?,
    };
    let view = root::read_view();
    let entries = view.read_dir(path.as_str())?;
    for e in entries {
        let _ = reply_dir_entry(&e);
    }
    Ok(())
}

fn reply_dir_entry(e : &VfsDirEntry) -> Result<(), ()> {
    let kind = match e.node_type {
        VfsNodeType::File => "f",
        VfsNodeType::Directory => "d",
        VfsNodeType::Symlink => "l",
        VfsNodeType::Special => "s",
    };
    let line = format!("{} {}\n", kind, e.name);
    let _ = runtime_serial::with_default_uart(|uart| {
        let _ = uart.write_all(line.as_bytes());
    });
    Ok(())
}

fn do_stat(cwd : &str, arg : Option<&str>) -> Result<(), VfsError> {
    let path = resolve_against_cwd(cwd, arg)?;
    let view = root::read_view();
    let m = view.metadata(path.as_str())?;
    let line = format!("{:?} size={} mode={:#o}\n",
                       m.node_type, m.size, m.mode);
    let _ = runtime_serial::with_default_uart(|uart| {
        let _ = uart.write_all(line.as_bytes());
    });
    Ok(())
}

fn do_rm(cwd : &str, arg : Option<&str>) -> Result<(), VfsError> {
    let path = resolve_against_cwd(cwd, arg)?;
    let mut sess = mount::open_rw_session(VfsFsKind::Ext4)?;
    sess.unlink(path.as_str())?;
    Ok(())
}

fn do_exec(cwd : &str, arg : Option<&str>, _rest : &[&str]) -> Result<(), VfsError> {
    #[cfg(target_arch = "riscv64")]
    {
        let path = resolve_against_cwd(cwd, arg)?;
        let view = root::read_view();
        let m = view.metadata(path.as_str())?;
        if m.node_type != VfsNodeType::File {
            return Err(VfsError::NotAFile);
        }
        match mm::kernel_mm::from_elf_path(path.as_str()) {
            Ok(loaded) => {
                let tid = task::spawn_user_task_spec(
                    task::UserTask::new(loaded.entry_pc,
                                        task::AddressSpaceHandle::from_raw(loaded.satp),
                                        task::UserImageInfo::new(loaded.image_base,
                                                                 loaded.image_size),
                                        task::UserStack::from_range(loaded.stack_bottom,
                                                                    loaded.stack_top),
                                        loaded.user_aspace_ptr),
                );
                vfs::cwd::on_user_task_spawned(tid);
                task::wait_for_task_exit(tid);
                let code = task::reap_exited_task(tid).map(|e| {
                                                          vfs::cwd::drop_task_cwd(e.id);
                                                          vfs::fd::drop_task_fd_table(e.id);
                                                          e.exit_code
                                                      })
                                                      .unwrap_or(-1);
                let line = format!("exec exit_code={}\n", code);
                let _ = runtime_serial::with_default_uart(|uart| {
                    let _ = uart.write_all(line.as_bytes());
                });
                Ok(())
            }
            Err(e) => {
                let line = format!("exec load err: {:?}\n", e);
                let _ = runtime_serial::with_default_uart(|uart| {
                    let _ = uart.write_all(line.as_bytes());
                });
                Ok(())
            }
        }
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = cwd;
        let _ = arg;
        let _ = runtime_serial::with_default_uart(|uart| {
            let _ = uart.write_all(b"exec: unsupported on this arch\n");
        });
        Ok(())
    }
}
