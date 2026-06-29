# wateros-pseudo-shell — 公共 API

事实来源：`os/components/wateros-pseudo-shell/src/lib.rs`。

## 导出项

| 项 | 说明 |
|----|------|
| `run_pseudo_shell() -> !` | 进入 UART REPL；循环读行、解析命令、输出结果 |

## 内部辅助（未导出）

`read_line_into`、`do_cd`、`do_ls`、`do_stat`、`do_rm`、`do_exec`、`resolve_against_cwd`（经 `vfs`）、`reply_dir_entry`

## 常量

- `MAX_LINE = 512`：单行输入上限

## exec 路径（RISC-V）

1. `mm::kernel_mm::load_program_from_path`
2. `prepare_elf_user_stack`
3. `task::spawn_user_task_spec`
4. `vfs::cwd` / `mount_ns` / `cred::on_user_task_spawned`
5. `wait_for_task_exit` + `reap_exited_task`（清理 cred/cwd/mount_ns/fd）

## 依赖门面（非本 crate API）

调用方通过根内核 feature 启用 crate；无独立 `init()`。
