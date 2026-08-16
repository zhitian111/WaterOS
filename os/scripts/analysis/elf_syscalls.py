#!/usr/bin/env python3
"""静态审计 ELF 及其递归动态库依赖使用的 Linux syscall 候选集。

本工具只读输入文件。它结合反汇编中的 syscall 指令和未定义的 libc wrapper 符号，
给出保守的静态上界；运行时按条件选择、函数指针和 syscall(2) 的动态编号仍需 trace 验证。
"""

from __future__ import annotations

import argparse
import ast
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath
from typing import Iterable, Sequence


SCRIPT_DIR = Path(__file__).resolve().parent
OS_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(OS_ROOT / "scripts" / "source"))

from logging_utils import error, info, warning  # noqa: E402


COMPONENT = "ELF-SYSCALL"
NUMBER_SOURCE = (
    OS_ROOT
    / "components/wateros-syscall/syscall-api/api-v0/src/number.rs"
)
DISPATCH_SOURCE = (
    OS_ROOT
    / "components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs"
)
ELF_MAGIC = b"\x7fELF"


class AnalysisError(RuntimeError):
    """输入、工具链或 ELF 元数据无法可靠分析。"""


@dataclass(frozen=True)
class ElfMetadata:
    path: Path
    elf_class: str
    architecture: str
    needed: tuple[str, ...]
    search_paths: tuple[str, ...]
    interpreter: str | None
    soname: str | None


@dataclass(frozen=True)
class Evidence:
    method: str
    object_path: str
    location: str


@dataclass
class ObjectResult:
    path: Path
    architecture: str
    needed: list[str]
    resolved: dict[str, str] = field(default_factory=dict)
    unresolved: list[str] = field(default_factory=list)
    direct: list[tuple[int, str, str]] = field(default_factory=list)
    indirect: list[tuple[str, str]] = field(default_factory=list)
    wrapper_symbols: list[str] = field(default_factory=list)


@dataclass
class AnalysisResult:
    target: Path
    architecture: str
    objects: list[ObjectResult]
    syscalls: dict[int, list[Evidence]]
    indirect_sites: list[Evidence]
    unresolved: list[tuple[str, str]]
    names: dict[int, str]
    wateros_supported: set[int]


def run_tool(argv: Sequence[str]) -> str:
    completed = subprocess.run(argv, text=True, capture_output=True)
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        raise AnalysisError(
            f"command failed ({completed.returncode}): {' '.join(argv)}"
            + (f"\n{detail}" if detail else "")
        )
    return completed.stdout


def choose_tool(*names: str) -> str:
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    raise AnalysisError(f"required tool not found: {' or '.join(names)}")


def require_elf(path: Path) -> None:
    try:
        with path.open("rb") as source:
            magic = source.read(4)
    except OSError as exc:
        raise AnalysisError(f"cannot read input {path}: {exc}") from exc
    if magic != ELF_MAGIC:
        raise AnalysisError(f"not an ELF file: {path}")


def normalize_architecture(machine: str, elf_class: str) -> str:
    lowered = machine.lower()
    if "risc-v" in lowered:
        return "riscv64" if elf_class == "ELF64" else "riscv32"
    if "loongarch" in lowered:
        return "loongarch64" if elf_class == "ELF64" else "loongarch32"
    if "x86-64" in lowered or "advanced micro devices x86-64" in lowered:
        return "x86_64"
    if "aarch64" in lowered:
        return "aarch64"
    return machine.strip().replace(" ", "_").lower()


def parse_elf_header(text: str) -> tuple[str, str]:
    class_match = re.search(r"^\s*Class:\s+(\S+)", text, re.MULTILINE)
    machine_match = re.search(r"^\s*Machine:\s+(.+?)\s*$", text, re.MULTILINE)
    if not class_match or not machine_match:
        raise AnalysisError("readelf output is missing ELF class or machine")
    elf_class = class_match.group(1)
    return elf_class, normalize_architecture(machine_match.group(1), elf_class)


def parse_dynamic_section(text: str) -> tuple[list[str], list[str], str | None]:
    needed = re.findall(r"\(NEEDED\).*?\[([^]]+)]", text)
    search_paths: list[str] = []
    for match in re.finditer(r"\((?:RPATH|RUNPATH)\).*?\[([^]]*)]", text):
        search_paths.extend(part for part in match.group(1).split(":") if part)
    soname_match = re.search(r"\(SONAME\).*?\[([^]]+)]", text)
    return needed, search_paths, soname_match.group(1) if soname_match else None


def parse_interpreter(text: str) -> str | None:
    match = re.search(r"Requesting program interpreter:\s*([^]]+)]", text)
    return match.group(1).strip() if match else None


def inspect_elf(path: Path, readelf: str) -> ElfMetadata:
    require_elf(path)
    header = run_tool([readelf, "-h", str(path)])
    elf_class, architecture = parse_elf_header(header)
    dynamic = run_tool([readelf, "-dW", str(path)])
    needed, search_paths, soname = parse_dynamic_section(dynamic)
    program_headers = run_tool([readelf, "-lW", str(path)])
    return ElfMetadata(
        path=path,
        elf_class=elf_class,
        architecture=architecture,
        needed=tuple(needed),
        search_paths=tuple(search_paths),
        interpreter=parse_interpreter(program_headers),
        soname=soname,
    )


def _path_parts(path: PurePosixPath) -> list[str]:
    return [part for part in path.parts if part not in ("", "/", ".")]


def resolve_guest_path(root: Path, guest_path: str, *, max_links: int = 40) -> Path | None:
    """按 chroot 语义解析 root 内的绝对 symlink，避免跳到宿主文件系统。"""

    root = root.resolve()
    pending = _path_parts(PurePosixPath("/") / guest_path.lstrip("/"))
    resolved: list[str] = []
    links = 0
    while pending:
        part = pending.pop(0)
        if part == "..":
            if resolved:
                resolved.pop()
            continue
        candidate = root.joinpath(*resolved, part)
        if candidate.is_symlink():
            links += 1
            if links > max_links:
                return None
            target = PurePosixPath(os.readlink(candidate))
            if target.is_absolute():
                resolved = []
            pending = _path_parts(target) + pending
        else:
            resolved.append(part)
    candidate = root.joinpath(*resolved)
    return candidate if candidate.is_file() else None


def expand_search_path(value: str, origin: Path) -> str:
    return value.replace("${ORIGIN}", str(origin)).replace("$ORIGIN", str(origin))


def _candidate_from_directory(
    directory: str, needed: str, root: Path | None, origin: Path
) -> Path | None:
    expanded = expand_search_path(directory, origin)
    directory_path = Path(expanded)
    if root is not None and directory_path.is_absolute():
        try:
            guest_directory = "/" + str(directory_path.relative_to(root))
        except ValueError:
            guest_directory = expanded
        guest = str(PurePosixPath(guest_directory) / needed)
        candidate = resolve_guest_path(root, guest)
    else:
        candidate_path = directory_path / needed
        try:
            candidate = candidate_path.resolve(strict=True)
        except OSError:
            candidate = None
    return candidate if candidate and candidate.is_file() else None


def standard_library_paths(architecture: str) -> list[str]:
    paths = ["/lib", "/usr/lib", "/lib64", "/usr/lib64"]
    triples = {
        "x86_64": ("x86_64-linux-gnu",),
        "aarch64": ("aarch64-linux-gnu",),
        "riscv64": ("riscv64-linux-gnu",),
        "loongarch64": ("loongarch64-linux-gnu",),
    }
    for triple in triples.get(architecture, ()):
        paths.extend((f"/lib/{triple}", f"/usr/lib/{triple}"))
    return paths


def resolve_dependency(
    name: str,
    requester: ElfMetadata,
    *,
    root: Path | None,
    library_paths: Sequence[str],
) -> Path | None:
    if "/" in name:
        if root is not None and name.startswith("/"):
            return resolve_guest_path(root, name)
        candidate = (requester.path.parent / name).resolve()
        return candidate if candidate.is_file() else None

    directories = [
        str(requester.path.parent),
        *requester.search_paths,
        *library_paths,
        *standard_library_paths(requester.architecture),
    ]
    seen: set[str] = set()
    for directory in directories:
        if directory in seen:
            continue
        seen.add(directory)
        candidate = _candidate_from_directory(
            directory, name, root, requester.path.parent
        )
        if candidate is not None:
            return candidate

    # musl 的动态加载器本身就是 libc。精简 rootfs 可以不安装 libc.so symlink，
    # 但动态对象的 DT_NEEDED 仍会保留 libc.so。
    if name == "libc.so":
        loader_names = {
            "riscv64": "ld-musl-riscv64.so.1",
            "loongarch64": "ld-musl-loongarch64.so.1",
            "aarch64": "ld-musl-aarch64.so.1",
            "x86_64": "ld-musl-x86_64.so.1",
        }
        loader_name = loader_names.get(requester.architecture)
        if requester.interpreter and Path(requester.interpreter).name.startswith("ld-musl-"):
            if root is not None:
                candidate = resolve_guest_path(root, requester.interpreter)
            else:
                candidate_path = Path(requester.interpreter)
                candidate = candidate_path if candidate_path.is_file() else None
            if candidate is not None:
                return candidate
        if root is not None and loader_name:
            candidate = resolve_guest_path(root, f"/lib/{loader_name}")
            if candidate is not None:
                return candidate
    return None


def dependency_closure(
    target: Path,
    readelf: str,
    *,
    root: Path | None,
    library_paths: Sequence[str],
    recursive: bool,
) -> tuple[list[ElfMetadata], list[tuple[str, str]]]:
    queue = [inspect_elf(target, readelf)]
    objects: list[ElfMetadata] = []
    unresolved: list[tuple[str, str]] = []
    seen: set[Path] = set()
    expected_architecture = queue[0].architecture

    while queue:
        metadata = queue.pop(0)
        identity = metadata.path.resolve()
        if identity in seen:
            continue
        if metadata.architecture != expected_architecture:
            raise AnalysisError(
                f"architecture mismatch: {metadata.path} is {metadata.architecture}, "
                f"expected {expected_architecture}"
            )
        seen.add(identity)
        objects.append(metadata)
        dependency_names = list(metadata.needed)
        if metadata.interpreter:
            dependency_names.insert(0, metadata.interpreter)
        for name in dependency_names:
            dependency = resolve_dependency(
                name,
                metadata,
                root=root,
                library_paths=library_paths,
            )
            if dependency is None:
                unresolved.append((str(metadata.path), name))
                continue
            dependency_identity = dependency.resolve()
            if recursive and dependency_identity not in seen:
                queue.append(inspect_elf(dependency, readelf))
    return objects, unresolved


def parse_undefined_symbols(text: str) -> list[str]:
    symbols: list[str] = []
    pattern = re.compile(
        r"^\s*\d+:\s+\S+\s+\d+\s+(?:FUNC|IFUNC|NOTYPE)\s+"
        r"\S+\s+\S+\s+UND\s+(\S+)",
        re.MULTILINE,
    )
    for match in pattern.finditer(text):
        symbol = match.group(1).split("@", 1)[0]
        if symbol:
            symbols.append(symbol)
    return sorted(set(symbols))


def _parse_integer(value: str) -> int | None:
    value = value.strip().lstrip("#").lstrip("$")
    try:
        return int(value, 0)
    except ValueError:
        return None


def _instruction(line: str) -> tuple[str, str, str] | None:
    match = re.match(r"^\s*([0-9a-fA-F]+):\s+(\S+)(?:\s+(.*?))?\s*$", line)
    if not match:
        return None
    return match.group(1), match.group(2).lower(), match.group(3) or ""


def _split_operands(operands: str) -> list[str]:
    return [part.strip() for part in operands.split(",")]


def _register_name(value: str) -> str:
    return value.strip().lower().lstrip("%$")


def _syscall_instruction(architecture: str, mnemonic: str) -> bool:
    if architecture == "riscv64":
        return mnemonic == "ecall"
    if architecture == "loongarch64":
        return mnemonic == "syscall"
    if architecture == "x86_64":
        return mnemonic == "syscall"
    if architecture == "aarch64":
        return mnemonic == "svc"
    return False


def _update_syscall_register(
    architecture: str, mnemonic: str, operands: str, current: int | None
) -> tuple[int | None, bool]:
    ops = _split_operands(operands)
    if architecture == "riscv64":
        target = "a7"
        if not ops or _register_name(ops[0]) not in (target, "x17"):
            return current, False
        if mnemonic == "li" and len(ops) >= 2:
            return _parse_integer(ops[1]), True
        if mnemonic == "lui" and len(ops) >= 2:
            value = _parse_integer(ops[1])
            return (value << 12) if value is not None else None, True
        if mnemonic in ("addi", "addiw", "ori") and len(ops) >= 3:
            source = _register_name(ops[1])
            immediate = _parse_integer(ops[2])
            if source in ("zero", "x0"):
                return immediate, True
            if source in (target, "x17") and current is not None and immediate is not None:
                return (current | immediate) if mnemonic == "ori" else current + immediate, True
        return None, True

    if architecture == "loongarch64":
        if not ops or _register_name(ops[0]) not in ("a7", "r11"):
            return current, False
        if mnemonic in ("li.w", "li.d") and len(ops) >= 2:
            return _parse_integer(ops[1]), True
        if mnemonic in ("addi.w", "addi.d", "ori") and len(ops) >= 3:
            source = _register_name(ops[1])
            immediate = _parse_integer(ops[2])
            if source in ("zero", "r0"):
                return immediate, True
            if source in ("a7", "r11") and current is not None and immediate is not None:
                return (current | immediate) if mnemonic == "ori" else current + immediate, True
        if mnemonic == "lu12i.w" and len(ops) >= 2:
            value = _parse_integer(ops[1])
            return (value << 12) if value is not None else None, True
        return None, True

    if architecture == "x86_64":
        if not ops or _register_name(ops[-1]) not in ("eax", "rax"):
            return current, False
        if mnemonic.startswith("mov") and len(ops) >= 2:
            return _parse_integer(ops[0]), True
        if mnemonic.startswith("xor") and len(ops) >= 2 and ops[0] == ops[1]:
            return 0, True
        return None, True

    if architecture == "aarch64":
        if not ops or _register_name(ops[0]) not in ("w8", "x8"):
            return current, False
        if mnemonic in ("mov", "movz") and len(ops) >= 2:
            return _parse_integer(ops[1]), True
        return None, True

    return current, False


def parse_disassembly(
    text: str, architecture: str
) -> tuple[list[tuple[int, str, str]], list[tuple[str, str]]]:
    direct: list[tuple[int, str, str]] = []
    indirect: list[tuple[str, str]] = []
    current_function = "<unknown>"
    syscall_number: int | None = None
    function_pattern = re.compile(r"^\s*[0-9a-fA-F]+\s+<([^>]+)>:\s*$")

    for line in text.splitlines():
        function_match = function_pattern.match(line)
        if function_match:
            current_function = function_match.group(1)
            syscall_number = None
            continue
        parsed = _instruction(line)
        if parsed is None:
            continue
        address, mnemonic, operands = parsed
        if _syscall_instruction(architecture, mnemonic):
            if syscall_number is None:
                indirect.append((address, current_function))
            elif syscall_number >= 0:
                direct.append((syscall_number, address, current_function))
            continue
        syscall_number, wrote = _update_syscall_register(
            architecture, mnemonic, operands, syscall_number
        )
        if mnemonic.startswith(("call", "jal", "bl")) and not wrote:
            syscall_number = None
    return direct, indirect


class MacroEvaluator(ast.NodeVisitor):
    def __init__(self, macros: dict[str, str]):
        self.macros = macros
        self.cache: dict[str, int] = {}
        self.active: set[str] = set()

    def macro(self, name: str) -> int:
        if name in self.cache:
            return self.cache[name]
        if name in self.active or name not in self.macros:
            raise ValueError(name)
        self.active.add(name)
        try:
            expression = self.macros[name]
            value = self.visit(ast.parse(expression, mode="eval").body)
            self.cache[name] = value
            return value
        finally:
            self.active.remove(name)

    def visit_Constant(self, node: ast.Constant) -> int:
        if isinstance(node.value, int):
            return node.value
        raise ValueError(node.value)

    def visit_Name(self, node: ast.Name) -> int:
        return self.macro(node.id)

    def visit_BinOp(self, node: ast.BinOp) -> int:
        left, right = self.visit(node.left), self.visit(node.right)
        if isinstance(node.op, ast.Add):
            return left + right
        if isinstance(node.op, ast.Sub):
            return left - right
        if isinstance(node.op, ast.LShift):
            return left << right
        if isinstance(node.op, ast.BitOr):
            return left | right
        raise ValueError(type(node.op).__name__)

    def visit_UnaryOp(self, node: ast.UnaryOp) -> int:
        value = self.visit(node.operand)
        if isinstance(node.op, ast.USub):
            return -value
        if isinstance(node.op, ast.UAdd):
            return value
        raise ValueError(type(node.op).__name__)

    def generic_visit(self, node: ast.AST) -> int:
        raise ValueError(type(node).__name__)


def parse_syscall_header(text: str) -> dict[int, str]:
    macros: dict[str, str] = {}
    for line in text.splitlines():
        match = re.match(r"\s*#\s*define\s+(__NR\w+)\s+(.+?)\s*$", line)
        if not match or "(" in match.group(1):
            continue
        expression = re.sub(r"/\*.*?\*/|//.*", "", match.group(2)).strip()
        expression = re.sub(r"(?<=\d)[uUlL]+\b", "", expression)
        macros[match.group(1)] = expression
    evaluator = MacroEvaluator(macros)
    candidates: dict[int, list[str]] = {}
    for macro in macros:
        if macro in ("__NR_syscalls", "__NR_arch_specific_syscall"):
            continue
        try:
            number = evaluator.macro(macro)
        except (SyntaxError, ValueError):
            continue
        name = macro.removeprefix("__NR_")
        if name.startswith("3264_"):
            name = name.removeprefix("3264_")
        candidates.setdefault(number, []).append(name)

    names: dict[int, str] = {}
    for number, variants in candidates.items():
        variants.sort(key=lambda name: (name.startswith("3264_"), len(name), name))
        names[number] = variants[0]
    return names


def parse_wateros_numbers(text: str) -> tuple[dict[int, str], dict[str, int]]:
    by_number: dict[int, str] = {}
    by_constant: dict[str, int] = {}
    aliases = {
        "FORK": "clone",
        "EXEC": "execve",
        "GET_TIME": "gettimeofday",
        "WAITPID": "wait4",
        "FACESSAT2": "faccessat2",
    }
    pattern = re.compile(r"^pub const (\w+)\s*:\s*usize\s*=\s*(\d+)\s*;", re.MULTILINE)
    for constant, raw_number in pattern.findall(text):
        number = int(raw_number)
        by_constant[constant] = number
        by_number.setdefault(number, aliases.get(constant, constant.lower()))
    return by_number, by_constant


def wateros_support() -> tuple[dict[int, str], set[int]]:
    if not NUMBER_SOURCE.is_file() or not DISPATCH_SOURCE.is_file():
        return {}, set()
    names, constants = parse_wateros_numbers(NUMBER_SOURCE.read_text(encoding="utf-8"))
    dispatch = DISPATCH_SOURCE.read_text(encoding="utf-8")
    used = set(re.findall(r"api_v0::(\w+)\s*=>", dispatch))
    used.update(re.findall(r"table\[api_v0::(\w+)]", dispatch))
    return names, {constants[name] for name in used if name in constants}


def syscall_header_candidates(architecture: str, root: Path | None) -> list[Path]:
    relative: tuple[str, ...]
    if architecture == "x86_64":
        relative = (
            "usr/include/asm/unistd_64.h",
            "usr/include/x86_64-linux-gnu/asm/unistd_64.h",
        )
    else:
        relative = ("usr/include/asm-generic/unistd.h",)
    paths: list[Path] = []
    if root is not None:
        paths.extend(root / path for path in relative)
    paths.extend(Path("/") / path for path in relative)
    return paths


def load_syscall_names(
    architecture: str, root: Path | None, explicit_header: Path | None
) -> tuple[dict[int, str], set[int]]:
    wateros_names, supported = wateros_support()
    if explicit_header is not None and not explicit_header.is_file():
        raise AnalysisError(f"syscall header does not exist: {explicit_header}")
    candidates = [explicit_header] if explicit_header else syscall_header_candidates(architecture, root)
    for candidate in candidates:
        if candidate and candidate.is_file():
            names = parse_syscall_header(candidate.read_text(encoding="utf-8"))
            if names:
                return {**wateros_names, **names}, supported
    warning(
        "Linux syscall header not found; names are limited to WaterOS constants",
        component=COMPONENT,
    )
    return wateros_names, supported


SYMBOL_ALIASES = {
    "_exit": "exit_group",
    "exit": "exit_group",
    "execv": "execve",
    "execvp": "execve",
    "execvpe": "execve",
    "faccessat": "faccessat",
    "fork": "clone",
    "open": "openat",
    "open64": "openat",
    "pipe": "pipe2",
    "poll": "ppoll",
    "select": "pselect6",
    "stat": "newfstatat",
    "lstat": "newfstatat",
    "wait": "wait4",
    "waitpid": "wait4",
}


def wrapper_number(symbol: str, names: dict[int, str]) -> int | None:
    by_name = {name: number for number, name in names.items()}
    normalized = symbol.removeprefix("__libc_").removeprefix("__")
    normalized = SYMBOL_ALIASES.get(normalized, normalized)
    return by_name.get(normalized)


def is_indirect_syscall_wrapper(symbol: str) -> bool:
    normalized = symbol.removeprefix("__libc_").lstrip("_")
    return normalized in ("syscall", "syscall_cp", "syscall_cp_c")


def analyze(
    target: Path,
    *,
    root: Path | None,
    library_paths: Sequence[str],
    recursive: bool,
    include_symbols: bool,
    syscall_header: Path | None,
) -> AnalysisResult:
    readelf = choose_tool("llvm-readelf", "readelf")
    objdump = choose_tool("llvm-objdump", "objdump")
    target = target.resolve()
    root = root.resolve() if root else None
    if root is not None and not root.is_dir():
        raise AnalysisError(f"rootfs directory does not exist: {root}")
    metadata, unresolved = dependency_closure(
        target,
        readelf,
        root=root,
        library_paths=library_paths,
        recursive=recursive,
    )
    architecture = metadata[0].architecture
    names, supported = load_syscall_names(architecture, root, syscall_header)
    syscalls: dict[int, list[Evidence]] = {}
    indirect_sites: list[Evidence] = []
    results: list[ObjectResult] = []

    for item in metadata:
        result = ObjectResult(item.path, item.architecture, list(item.needed))
        dependency_names = list(item.needed)
        if item.interpreter:
            dependency_names.insert(0, item.interpreter)
        for name in dependency_names:
            resolved = resolve_dependency(
                name, item, root=root, library_paths=library_paths
            )
            if resolved is None:
                result.unresolved.append(name)
            else:
                result.resolved[name] = str(resolved)

        disassembly = run_tool([objdump, "-d", "--no-show-raw-insn", str(item.path)])
        direct, indirect = parse_disassembly(disassembly, architecture)
        result.direct = direct
        result.indirect = indirect
        for number, address, function in direct:
            evidence = Evidence("instruction", str(item.path), f"{function}@0x{address}")
            syscalls.setdefault(number, []).append(evidence)
        for address, function in indirect:
            indirect_sites.append(
                Evidence("indirect-instruction", str(item.path), f"{function}@0x{address}")
            )

        if include_symbols:
            symbol_text = run_tool([readelf, "--dyn-syms", "--wide", str(item.path)])
            result.wrapper_symbols = parse_undefined_symbols(symbol_text)
            for symbol in result.wrapper_symbols:
                if is_indirect_syscall_wrapper(symbol):
                    indirect_sites.append(
                        Evidence("indirect-wrapper-symbol", str(item.path), symbol)
                    )
                number = wrapper_number(symbol, names)
                if number is None:
                    continue
                evidence = Evidence("wrapper-symbol", str(item.path), symbol)
                syscalls.setdefault(number, []).append(evidence)
        results.append(result)

    return AnalysisResult(
        target=target,
        architecture=architecture,
        objects=results,
        syscalls=syscalls,
        indirect_sites=indirect_sites,
        unresolved=unresolved,
        names=names,
        wateros_supported=supported,
    )


def relative_display(path: str, root: Path | None) -> str:
    if root is None:
        return path
    try:
        return "/" + str(Path(path).resolve().relative_to(root.resolve()))
    except ValueError:
        return path


def text_report(
    result: AnalysisResult, root: Path | None, *, show_evidence: bool = False
) -> str:
    lines = [
        f"Target: {result.target}",
        f"Architecture: {result.architecture}",
        f"ELF objects: {len(result.objects)}",
        "",
        "Dependencies:",
    ]
    for index, item in enumerate(result.objects):
        lines.append(f"  [{index}] {relative_display(str(item.path), root)}")
    if result.unresolved:
        lines.extend(("", "Unresolved dependencies:"))
        for requester, name in result.unresolved:
            lines.append(f"  {name} (needed by {relative_display(requester, root)})")

    lines.extend(("", "Syscall candidates:"))
    if not result.syscalls:
        lines.append("  (none found)")
    wateros_arch = result.architecture in ("riscv64", "loongarch64")
    for number in sorted(result.syscalls):
        name = result.names.get(number, "unknown")
        if wateros_arch:
            status = "implemented" if number in result.wateros_supported else "missing"
        else:
            status = "n/a"
        methods = sorted({evidence.method for evidence in result.syscalls[number]})
        lines.append(
            f"  {number:>4}  {name:<24} wateros={status:<11} via={','.join(methods)}"
        )
        if show_evidence:
            for evidence in result.syscalls[number]:
                lines.append(
                    "        "
                    f"{relative_display(evidence.object_path, root)}: {evidence.location}"
                )

    lines.extend(("", "Indirect syscall sites:"))
    if not result.indirect_sites:
        lines.append("  (none found)")
    for evidence in result.indirect_sites:
        lines.append(
            f"  {relative_display(evidence.object_path, root)}: {evidence.location} "
            f"via={evidence.method}"
        )
    lines.extend(
        (
            "",
            "Note: this is a conservative static inventory, not a runtime trace. ",
            "Conditional paths may be included; indirect syscall numbers cannot be recovered.",
        )
    )
    return "\n".join(lines)


def json_report(result: AnalysisResult, root: Path | None) -> str:
    wateros_arch = result.architecture in ("riscv64", "loongarch64")
    payload = {
        "schema": 1,
        "target": str(result.target),
        "architecture": result.architecture,
        "objects": [
            {
                "path": relative_display(str(item.path), root),
                "needed": item.needed,
                "resolved": {
                    name: relative_display(path, root)
                    for name, path in sorted(item.resolved.items())
                },
                "unresolved": item.unresolved,
            }
            for item in result.objects
        ],
        "syscalls": [
            {
                "number": number,
                "name": result.names.get(number),
                "wateros": (
                    "implemented" if number in result.wateros_supported else "missing"
                )
                if wateros_arch
                else "not-applicable",
                "evidence": [
                    {
                        "method": evidence.method,
                        "object": relative_display(evidence.object_path, root),
                        "location": evidence.location,
                    }
                    for evidence in result.syscalls[number]
                ],
            }
            for number in sorted(result.syscalls)
        ],
        "indirect_syscall_sites": [
            {
                "method": evidence.method,
                "object": relative_display(evidence.object_path, root),
                "location": evidence.location,
            }
            for evidence in result.indirect_sites
        ],
        "unresolved_dependencies": [
            {"requester": relative_display(requester, root), "name": name}
            for requester, name in result.unresolved
        ],
        "limitations": [
            "The result is a conservative static inventory, not a runtime trace.",
            "Conditional paths may be included.",
            "Indirect syscall numbers cannot be recovered statically.",
        ],
    }
    return json.dumps(payload, indent=2, sort_keys=True)


def parse_library_paths(values: Iterable[str]) -> list[str]:
    paths: list[str] = []
    for value in values:
        paths.extend(part for part in value.split(":") if part)
    return paths


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="递归分析 ELF 可执行文件/动态库的 Linux syscall 静态候选集。",
        epilog=(
            "示例: ./scripts/analysis/elf_syscalls.py --root ../user/build/staging/rv/rootfs "
            "../user/build/staging/rv/rootfs/bin/busybox"
        ),
    )
    parser.add_argument("target", type=Path, help="待分析的 ELF 可执行文件或动态库")
    parser.add_argument(
        "--root",
        type=Path,
        help="目标 rootfs；绝对 DT_NEEDED/interpreter 路径按 chroot 语义解析",
    )
    parser.add_argument(
        "-L",
        "--library-path",
        action="append",
        default=[],
        metavar="DIR[:DIR...]",
        help="补充动态库搜索目录，可重复指定并支持冒号分隔",
    )
    parser.add_argument(
        "--no-recursive", action="store_true", help="只分析输入 ELF，不递归解析动态库"
    )
    parser.add_argument(
        "--no-symbol-candidates",
        action="store_true",
        help="不从未定义 libc wrapper 符号补充 syscall 候选",
    )
    parser.add_argument(
        "--syscall-header",
        type=Path,
        help="显式指定含 __NR_* 定义的 Linux unistd 头文件",
    )
    parser.add_argument(
        "--format", choices=("text", "json"), default="text", help="输出格式"
    )
    parser.add_argument(
        "--show-evidence",
        action="store_true",
        help="文本输出中显示每个 syscall 的 ELF、符号或指令地址证据",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="存在未解析依赖或动态 syscall 编号时返回非零状态",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        result = analyze(
            args.target,
            root=args.root,
            library_paths=parse_library_paths(args.library_path),
            recursive=not args.no_recursive,
            include_symbols=not args.no_symbol_candidates,
            syscall_header=args.syscall_header,
        )
    except AnalysisError as exc:
        error(str(exc), component=COMPONENT)
        return 2

    if args.format == "json":
        print(json_report(result, args.root))
    else:
        print(text_report(result, args.root, show_evidence=args.show_evidence))
    info(
        f"analysis complete objects={len(result.objects)} syscalls={len(result.syscalls)} "
        f"indirect={len(result.indirect_sites)} unresolved={len(result.unresolved)}",
        component=COMPONENT,
    )
    if args.strict and (result.unresolved or result.indirect_sites):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
