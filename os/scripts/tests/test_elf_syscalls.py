"""验证 ELF syscall 静态分析器的纯解析与 rootfs 路径语义。"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path


ANALYSIS_SCRIPTS = Path(__file__).resolve().parents[1] / "analysis"
sys.path.insert(0, str(ANALYSIS_SCRIPTS))

import elf_syscalls


class MetadataParserTests(unittest.TestCase):
    def test_dynamic_section(self) -> None:
        text = """
 0x0000000000000001 (NEEDED) Shared library: [libc.so]
 0x000000000000001d (RUNPATH) Library runpath: [$ORIGIN/../lib:/opt/lib]
 0x000000000000000e (SONAME) Library soname: [libdemo.so.1]
"""
        needed, paths, soname = elf_syscalls.parse_dynamic_section(text)
        self.assertEqual(needed, ["libc.so"])
        self.assertEqual(paths, ["$ORIGIN/../lib", "/opt/lib"])
        self.assertEqual(soname, "libdemo.so.1")

    def test_undefined_versioned_symbols(self) -> None:
        text = """
  1: 0000000000000000 0 FUNC GLOBAL DEFAULT UND read@GLIBC_2.2.5
  2: 0000000000000000 0 OBJECT GLOBAL DEFAULT UND stdout
  3: 0000000000000000 0 NOTYPE WEAK DEFAULT UND syscall
"""
        self.assertEqual(
            elf_syscalls.parse_undefined_symbols(text), ["read", "syscall"]
        )


class SyscallParserTests(unittest.TestCase):
    def test_generic_header_aliases_and_expressions(self) -> None:
        names = elf_syscalls.parse_syscall_header(
            """
#define __NR_read 63
#define __NR_arch_specific_syscall 244
#define __NR_riscv_hwprobe (__NR_arch_specific_syscall + 14)
#define __NR3264_fcntl 25
#define __NR_fcntl __NR3264_fcntl
"""
        )
        self.assertEqual(names[63], "read")
        self.assertEqual(names[258], "riscv_hwprobe")
        self.assertEqual(names[25], "fcntl")

    def test_riscv_direct_and_indirect_sites(self) -> None:
        direct, indirect = elf_syscalls.parse_disassembly(
            """
0000000000010000 <read>:
  10000: li a7, 0x3f
  10004: ecall
0000000000010010 <syscall>:
  10010: mv a7, a0
  10014: ecall
""",
            "riscv64",
        )
        self.assertEqual(direct, [(63, "10004", "read")])
        self.assertEqual(indirect, [("10014", "syscall")])

    def test_loongarch_direct_site(self) -> None:
        direct, indirect = elf_syscalls.parse_disassembly(
            """
0000000000001000 <read>:
  1000: li.d $a7, 0x3f
  1004: syscall 0
""",
            "loongarch64",
        )
        self.assertEqual(direct, [(63, "1004", "read")])
        self.assertEqual(indirect, [])

    def test_x86_direct_site(self) -> None:
        direct, indirect = elf_syscalls.parse_disassembly(
            """
0000000000001000 <exit>:
  1000: movl $0x3c, %eax
  1005: syscall
""",
            "x86_64",
        )
        self.assertEqual(direct, [(60, "1005", "exit")])
        self.assertEqual(indirect, [])

    def test_generic_syscall_wrapper_is_indirect(self) -> None:
        self.assertTrue(elf_syscalls.is_indirect_syscall_wrapper("syscall"))
        self.assertTrue(elf_syscalls.is_indirect_syscall_wrapper("__syscall_cp"))
        self.assertFalse(elf_syscalls.is_indirect_syscall_wrapper("read"))

    def test_text_report_hides_evidence_by_default(self) -> None:
        evidence = elf_syscalls.Evidence("instruction", "/bin/demo", "read@0x1000")
        result = elf_syscalls.AnalysisResult(
            target=Path("/bin/demo"),
            architecture="riscv64",
            objects=[],
            syscalls={63: [evidence]},
            indirect_sites=[],
            unresolved=[],
            names={63: "read"},
            wateros_supported={63},
        )
        self.assertNotIn("read@0x1000", elf_syscalls.text_report(result, None))
        self.assertIn(
            "read@0x1000",
            elf_syscalls.text_report(result, None, show_evidence=True),
        )


class RootfsResolutionTests(unittest.TestCase):
    def test_absolute_symlink_stays_inside_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "usr/lib").mkdir(parents=True)
            (root / "usr/lib/libdemo.so.1").write_bytes(b"ELF")
            (root / "lib").symlink_to("/usr/lib")
            resolved = elf_syscalls.resolve_guest_path(root, "/lib/libdemo.so.1")
            self.assertEqual(resolved, root / "usr/lib/libdemo.so.1")

    def test_relative_parent_symlink_stays_inside_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "usr/lib").mkdir(parents=True)
            (root / "usr/lib/libdemo.so.1").write_bytes(b"ELF")
            (root / "lib").mkdir()
            (root / "lib/libdemo.so").symlink_to("../usr/lib/libdemo.so.1")
            resolved = elf_syscalls.resolve_guest_path(root, "/lib/libdemo.so")
            self.assertEqual(resolved, root / "usr/lib/libdemo.so.1")

    def test_root_backed_origin_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / "opt/app/bin"
            library = root / "opt/app/lib/libdemo.so"
            origin.mkdir(parents=True)
            library.parent.mkdir(parents=True)
            library.write_bytes(b"ELF")
            resolved = elf_syscalls._candidate_from_directory(
                "$ORIGIN/../lib", "libdemo.so", root, origin
            )
            self.assertEqual(resolved, library)


if __name__ == "__main__":
    unittest.main()
