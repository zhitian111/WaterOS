//! Shebang 脚本解析与 exec argv 重组（纯逻辑，不依赖 VFS）。
//!
//! 当目标文件非 ELF 且判定为文本脚本时，由 mm-impl 调用本模块解析 `#!` 并重组 argv，
//! 再加载解释器 ELF。首版不支持 `#!/usr/bin/env` 的 PATH 搜索。

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// shebang 探测窗口上限（与 Linux `BINPRM_BUF_SIZE` 量级一致）；超出窗口的首行内容不参与格式判定。
pub const SHEBANG_PROBE_MAX : usize = 256;

/// 解释器链递归深度上限（防环）；达到上限必须返回错误而不能继续消耗内核栈或文件句柄。
pub const MAX_INTERPRETER_RECURSION : usize = 4;

/// 非 ELF 脚本解析失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecResolveError {
    /// 非 ELF 且非可执行文本脚本（无 shebang 或含二进制 NUL 等）。
    NotExecutable,
    /// shebang 行非法或解释器路径为空。
    InvalidShebang,
    /// 解释器链递归过深。
    RecursionLimit,
}

/// 解析成功的 shebang 内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedShebang {
    /// 解释器路径（shebang 行第一个 token），未做 PATH 搜索且不含首行 `#!` 前缀。
    pub interpreter : String,
    /// shebang 行其余 token（如 `sh`、`-x`）；仅按空白分隔，不能表达带空格的单个参数。
    pub args : Vec<String>,
}

/// 判断字节前缀是否为当前支持的 ELF64 小端格式；它只检查魔数、类别和字节序，不能证明文件可装载。
#[inline]
pub fn is_elf_prefix(data : &[u8]) -> bool {
    data.len() >= 6 && &data[0..4] == b"\x7FELF" && data[4] == 2 && data[5] == 1
}

/// 脚本探测字节：制表/空白/换行/回车，或可打印 ASCII（`0x20..=0x7E`），或
/// 高位字节（`>= 0x80`，UTF-8 多字节文本，如注释里的 `©`）。
///
/// 放宽高位字节以匹配 Linux `binfmt_script` 语义：它只要求首行是合法 shebang，
/// 并不要求探测窗口内整段为 7-bit ASCII。真实脚本（如 Debian `py3compile`）
/// 首 256 字节内常含非 ASCII 版权注释，若拒绝会导致 `ENOEXEC` 而按 shell 兜底执行。
#[inline]
fn is_shebang_line_byte(b : u8) -> bool {
    b == b'\t' || b == b' ' || b == b'\n' || b == b'\r' || (b >= 0x20 && b <= 0x7E) || b >= 0x80
}

/// 首行结束位置（不含 `\n`）；无换行则返回 `data.len()`。
fn shebang_line_end(data : &[u8]) -> usize {
    data.iter()
        .position(|&b| b == b'\n')
        .unwrap_or(data.len())
}

/// 跳过脚本开头 BOM 与空白（部分测试脚本在 shebang 或正文前有换行）。
/// 返回值仍借用输入，空输入或全空白输入会返回空切片。
pub fn skip_leading_script_whitespace(data : &[u8]) -> &[u8] {
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(b"\xef\xbb\xbf") {
            i += 3;
            continue;
        }
        match data[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            _ => break,
        }
    }
    &data[i..]
}

/// 探测窗口内是否为无 NUL 的可打印文本（非 ELF 时的脚本判定基础）。
/// 这是启发式判定，不代表文件一定可执行；真正的解释器解析和装载仍可能失败。
pub fn is_text_file(data : &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let probe_len = data.len()
                        .min(SHEBANG_PROBE_MAX);
    let probe = &data[..probe_len];
    if probe.contains(&0) {
        return false;
    }
    probe.iter()
         .all(|&b| is_shebang_line_byte(b))
}

/// 非 ELF 时判定是否为可执行的文本脚本（含无 shebang 的 shell 脚本正文）。
pub fn is_text_script_candidate(data : &[u8]) -> bool { is_text_file(data) }

/// 解析首行 shebang（调用前须已 [`skip_leading_script_whitespace`]）；失败返回错误。
pub fn parse_shebang_line_at(data : &[u8]) -> Result<ParsedShebang, ExecResolveError> {
    if data.len() < 2 || &data[0..2] != b"#!" {
        return Err(ExecResolveError::InvalidShebang);
    }
    let probe_len = data.len()
                        .min(SHEBANG_PROBE_MAX);
    if data[..probe_len].contains(&0) {
        return Err(ExecResolveError::NotExecutable);
    }
    let line_end = shebang_line_end(data).min(probe_len);
    if !data[..line_end].iter()
                        .all(|&b| is_shebang_line_byte(b))
    {
        return Err(ExecResolveError::InvalidShebang);
    }
    let line = &data[..line_end];
    let body = core::str::from_utf8(&line[2..]).map_err(|_| ExecResolveError::InvalidShebang)?;
    let mut tokens = body.split_whitespace();
    let interpreter = tokens.next()
                            .filter(|s| !s.is_empty())
                            .ok_or(ExecResolveError::InvalidShebang)?;
    let args = tokens.map(String::from)
                     .collect();
    Ok(ParsedShebang { interpreter : String::from(interpreter),
                       args })
}

/// 解析首行 shebang；失败返回 [`ExecResolveError`]。
pub fn parse_shebang_line(data : &[u8]) -> Result<ParsedShebang, ExecResolveError> {
    if !is_text_file(data) {
        return Err(ExecResolveError::NotExecutable);
    }
    parse_shebang_line_at(skip_leading_script_whitespace(data))
}

/// 根据脚本路径前缀（`/glibc/`、`/musl/`）返回同 libc 下的 busybox 路径。
pub fn busybox_path_for_script(script_path : &str) -> Option<&'static str> {
    if script_path.starts_with("/glibc/") {
        Some("/glibc/busybox")
    } else if script_path.starts_with("/musl/") {
        Some("/musl/busybox")
    } else {
        None
    }
}

/// 将镜像约定的 `#!/busybox` 映射到脚本所属 libc 目录。
/// 非测试盘约定路径保持原样，避免该兼容规则意外重写普通用户提供的解释器。
pub fn remap_interpreter_path(script_path : &str, interpreter : &str) -> String {
    if interpreter.starts_with("/glibc/") || interpreter.starts_with("/musl/") {
        return String::from(interpreter);
    }
    if let Some(busybox) = busybox_path_for_script(script_path) {
        if interpreter == "/busybox" {
            return String::from(busybox);
        }
    }
    String::from(interpreter)
}

/// 解析脚本的解释器与参数：有 shebang 则解析并 remap；无 shebang 则回退为
/// `/{glibc|musl}/busybox sh`（与测试盘脚本约定一致）。
pub fn resolve_script_interpreter(script_path : &str,
                                  data : &[u8])
                                  -> Result<(String, Vec<String>), ExecResolveError> {
    if !is_text_file(data) {
        return Err(ExecResolveError::NotExecutable);
    }
    let stripped = skip_leading_script_whitespace(data);
    if stripped.len() >= 2 && &stripped[0..2] == b"#!" {
        let parsed = parse_shebang_line_at(stripped)?;
        let interpreter = remap_interpreter_path(script_path, &parsed.interpreter);
        return Ok((interpreter, parsed.args));
    }
    let busybox = busybox_path_for_script(script_path).ok_or(ExecResolveError::NotExecutable)?;
    Ok((String::from(busybox), vec![String::from("sh")]))
}

/// 测试盘 busybox 为静态链接，`argv[0]` 须为 applet 名（如 `sh`），而非解释器完整路径。
#[inline]
fn is_busybox_interpreter(interpreter : &str) -> bool {
    interpreter.ends_with("/busybox") || interpreter == "busybox"
}

/// 组装解释器加载时的 argv。
///
/// - 普通 ELF：遵循 Linux binfmt_script（`argv[0]` = 解释器路径）。
/// - busybox：与 bring-up 约定一致（`argv[0]` = `sh` 等 applet 名）。
///
/// `user_argv` 的第一个元素是原脚本路径时会被替换；若为空也可工作，生成的列表仍含脚本路径。
pub fn build_interpreted_argv(script_path : &str,
                              interpreter : &str,
                              shebang_args : &[String],
                              user_argv : &[&str])
                              -> Vec<String> {
    if is_busybox_interpreter(interpreter) {
        let mut argv = Vec::with_capacity(1 + shebang_args.len() + user_argv.len());
        if shebang_args.is_empty() {
            argv.push(String::from("sh"));
        } else {
            argv.extend(shebang_args.iter()
                                    .cloned());
        }
        argv.push(String::from(script_path));
        if user_argv.len() > 1 {
            for arg in &user_argv[1..] {
                argv.push(String::from(*arg));
            }
        }
        return argv;
    }

    let mut argv = Vec::with_capacity(2 + shebang_args.len() + user_argv.len());
    argv.push(String::from(interpreter));
    argv.extend(shebang_args.iter()
                            .cloned());
    argv.push(String::from(script_path));
    if user_argv.len() > 1 {
        for arg in &user_argv[1..] {
            argv.push(String::from(*arg));
        }
    }
    argv
}

/// bring-up 自检：shebang 解析与 argv 重组。
pub fn test() {
    log::trace!("[mm-api][executable] test begin");
    let data = b"#!/glibc/busybox sh\n";
    let parsed = parse_shebang_line(data).expect("busybox shebang");
    assert_eq!(parsed.interpreter, "/glibc/busybox");
    assert_eq!(parsed.args, vec![String::from("sh")]);

    let crlf = b"#!/bin/sh -x\r\n";
    let parsed = parse_shebang_line(crlf).expect("crlf shebang");
    assert_eq!(parsed.interpreter, "/bin/sh");
    assert_eq!(parsed.args, vec![String::from("-x")]);

    let mut nul = vec![b'#', b'!', b'/'];
    nul.push(0);
    nul.extend_from_slice(b"bin/sh\n");
    assert!(!is_text_script_candidate(&nul));

    let (interp, args) = resolve_script_interpreter("/glibc/basic_testcode.sh",
                                                    b"./busybox echo hi\n").unwrap();
    assert_eq!(interp, "/glibc/busybox");
    assert_eq!(args, vec![String::from("sh")]);

    let (interp, args) = resolve_script_interpreter("/glibc/busybox_testcode.sh",
                                                    b"#!/busybox sh\n").unwrap();
    assert_eq!(interp, "/glibc/busybox");
    assert_eq!(args, vec![String::from("sh")]);

    let (interp, args) = resolve_script_interpreter("/glibc/unixbench_testcode.sh",
                                                    b"#!/bin/bash\n").unwrap();
    assert_eq!(interp, "/bin/bash");
    assert!(args.is_empty());

    let leading_nl = b"\n./busybox echo test\n";
    let (interp, _) = resolve_script_interpreter("/musl/basic_testcode.sh", leading_nl).unwrap();
    assert_eq!(interp, "/musl/busybox");

    let shebang_args = vec![String::from("sh")];
    let user = ["/glibc/basic_testcode.sh",
                "arg1"];
    let argv = build_interpreted_argv("/glibc/basic_testcode.sh",
                                      "/glibc/busybox",
                                      &shebang_args,
                                      &user);
    assert_eq!(argv, vec![
            String::from("sh"),
            String::from("/glibc/basic_testcode.sh"),
            String::from("arg1"),
        ]);

    let mut elf = b"\x7FELF".to_vec();
    elf.extend_from_slice(&[2, 1, 1, 0]);
    assert!(is_elf_prefix(&elf));
    assert!(!is_elf_prefix(b"#!/bin/sh\n"));
    log::trace!("[mm-api][executable] test end");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_busybox_sh() {
        let data = b"#!/glibc/busybox sh\n echo hi\n";
        let parsed = parse_shebang_line(data).unwrap();
        assert_eq!(parsed.interpreter, "/glibc/busybox");
        assert_eq!(parsed.args, vec!["sh"]);
    }

    #[test]
    fn parse_with_flag_and_crlf() {
        let data = b"#!/bin/sh -x\r\n";
        let parsed = parse_shebang_line(data).unwrap();
        assert_eq!(parsed.interpreter, "/bin/sh");
        assert_eq!(parsed.args, vec!["-x"]);
    }

    #[test]
    fn reject_nul_in_probe() {
        let mut data = vec![b'#', b'!', b'/'];
        data.push(0);
        data.extend_from_slice(b"bin/sh\n");
        assert!(!is_text_script_candidate(&data));
    }

    #[test]
    fn reject_plain_text_without_shebang() {
        let data = b"echo hello\n";
        assert!(is_text_script_candidate(data));
        assert_eq!(resolve_script_interpreter("/other/echo.sh", data),
                   Err(ExecResolveError::NotExecutable));
    }

    #[test]
    fn no_shebang_busybox_fallback() {
        let data = b"./busybox echo hi\n";
        let (interp, args) = resolve_script_interpreter("/glibc/basic_testcode.sh", data).unwrap();
        assert_eq!(interp, "/glibc/busybox");
        assert_eq!(args, vec![String::from("sh")]);
    }

    #[test]
    fn remap_busybox_shebang() {
        let data = b"#!/busybox sh\n";
        let (interp, args) = resolve_script_interpreter("/glibc/x.sh", data).unwrap();
        assert_eq!(interp, "/glibc/busybox");
        assert_eq!(args, vec![String::from("sh")]);
    }

    #[test]
    fn build_argv_order() {
        let shebang_args = vec![String::from("sh")];
        let user = ["/glibc/basic_testcode.sh",
                    "arg1"];
        let argv = build_interpreted_argv("/glibc/basic_testcode.sh",
                                          "/glibc/busybox",
                                          &shebang_args,
                                          &user);
        assert_eq!(argv, vec!["sh",
                              "/glibc/basic_testcode.sh",
                              "arg1",]);
    }

    #[test]
    fn build_argv_linux_interpreter() {
        let argv = build_interpreted_argv("/usr/bin/script",
                                          "/bin/sh",
                                          &[],
                                          &["/usr/bin/script"]);
        assert_eq!(argv, vec!["/bin/sh",
                              "/usr/bin/script"]);
    }

    #[test]
    fn elf_prefix() {
        let mut elf = b"\x7FELF".to_vec();
        elf.extend_from_slice(&[2, 1, 1, 0]);
        assert!(is_elf_prefix(&elf));
        assert!(!is_elf_prefix(b"#!/bin/sh\n"));
    }
}
