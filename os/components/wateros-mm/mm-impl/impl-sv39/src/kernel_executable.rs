//! 从根卷路径装载可执行文件：ELF 直载或 shebang 脚本解析后加载解释器。

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use api_v0::executable::{
    self, ExecResolveError, MAX_INTERPRETER_RECURSION,
};
use api_v0::kernel_bringup::{LoadProgramError, LoadedElf};

use crate::kernel_elf::{from_elf_bytes, read_path_bytes};

/// 装载 `path` 指向的程序：ELF 直载，或解析 shebang 后递归加载解释器。
///
/// 返回 `(LoadedElf, 最终 argv)`；脚本场景下 argv 已按 Linux binfmt_script 重组。
pub fn load_program_from_path(path: &str, argv: &[&str]) -> Result<(LoadedElf, Vec<String>), LoadProgramError> {
    load_program_from_path_rec(path, argv, 0)
}

fn load_program_from_path_rec(
    path: &str,
    argv: &[&str],
    depth: usize,
) -> Result<(LoadedElf, Vec<String>), LoadProgramError> {
    if depth >= MAX_INTERPRETER_RECURSION {
        return Err(LoadProgramError::Script(ExecResolveError::RecursionLimit));
    }

    let data = read_path_bytes(path).map_err(LoadProgramError::Elf)?;

    if executable::is_elf_prefix(&data) {
        let final_argv = argv_vec(path, argv);
        let loaded = from_elf_bytes(&data).map_err(LoadProgramError::Elf)?;
        return Ok((loaded, final_argv));
    }

    if !executable::is_text_script_candidate(&data) {
        return Err(LoadProgramError::Script(ExecResolveError::NotExecutable));
    }

    let (interpreter, shebang_args) = executable::resolve_script_interpreter(path, &data)
        .map_err(LoadProgramError::Script)?;
    let user_argv: Vec<&str> = if argv.is_empty() {
        vec![path]
    } else {
        argv.to_vec()
    };
    let new_argv =
        executable::build_interpreted_argv(path, &interpreter, &shebang_args, &user_argv);
    let new_argv_refs: Vec<&str> = new_argv.iter().map(String::as_str).collect();
    load_program_from_path_rec(&interpreter, &new_argv_refs, depth + 1)
}

fn argv_vec(path: &str, argv: &[&str]) -> Vec<String> {
    if argv.is_empty() {
        vec![String::from(path)]
    } else {
        argv.iter().map(|s| String::from(*s)).collect()
    }
}
