//! 启动期 boot 树遍历时的轻量内容探测：`.sh` 文本预览与可执行 ELF 头字段（非完整 loader）。
//! 本模块代码由AI完成

use alloc::string::String;

/// `.sh` 在启动日志中单次打印的最大字节数（避免大脚本刷屏）。
pub const MAX_SH_BOOT_BYTES: usize = 4096;

// 本方法代码由AI完成
fn parse_elf64_le_header(data: &[u8]) -> Option<(u8, u8, u16, u64)> {
    if data.len() < 0x40 {
        return None;
    }
    if &data[0..4] != b"\x7fELF" {
        return None;
    }
    let class = data[4];
    let data_enc = data[5];
    if class != 2 || data_enc != 1 {
        return None;
    }
    let machine = u16::from_le_bytes([data[18], data[19]]);
    let entry = u64::from_le_bytes(data.get(24..32)?.try_into().ok()?);
    Some((class, data_enc, machine, entry))
}

// 本方法代码由AI完成
fn name_ends_with_sh(name: &[u8]) -> bool {
    name.len() >= 12 && name[name.len() - 12..] == *b"_testcode.sh"
}

// 本方法代码由AI完成
fn is_unix_executable(mode: u16) -> bool {
    mode & (0o111 as u16) != 0
}

/// 若 `name` 以 `.sh` 结尾，将 `content` 以 UTF-8 有损预览写入日志。
// 本方法代码由AI完成
pub fn log_sh_file(path_display: &str, content: &[u8]) {
    let preview = String::from_utf8_lossy(content);
    let max_chars = 512usize;
    let shown = preview.chars().take(max_chars).collect::<String>();
    let truncated = preview.chars().count() > max_chars;
    logging::info!(
        "[fs::boot-tree][sh] path={} bytes={} utf8_preview={:?}{}",
        path_display,
        content.len(),
        shown,
        if truncated { " ...(truncated)" } else { "" }
    );
}

/// 对可执行普通文件：若前缀为 ELF64 LE，则打印 class/data/machine/entry。
// 本方法代码由AI完成
pub fn log_elf_exec_if_applicable(path_display: &str, prefix: &[u8], mode: u16) {
    if !is_unix_executable(mode) {
        return;
    }
    let Some((class, data, machine, entry)) = parse_elf64_le_header(prefix) else {
        return;
    };
    logging::info!(
        "[fs::boot-tree][elf-exec] path={} mode={:#o} class={} data={} machine={:#x} entry={:#x}",
        path_display,
        mode,
        class,
        data,
        machine,
        entry
    );
}

/// 供目录项判断：是否应按 shell 脚本路径打印正文。
// 本方法代码由AI完成
pub fn should_dump_sh_script(name: &[u8]) -> bool {
    name_ends_with_sh(name)
}
