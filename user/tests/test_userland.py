from __future__ import annotations

import hashlib
import tempfile
import unittest
import json
import subprocess
import sys
from pathlib import Path
from unittest import mock

from tools import userland


class ConfigurationTests(unittest.TestCase):
    def test_architecture_environment_override(self) -> None:
        with mock.patch.dict("os.environ", {"RV_CROSS_COMPILE": "custom-rv-"}):
            architecture = userland.load_architecture("rv")
        self.assertEqual(architecture.cross_compile, "custom-rv-")
        self.assertEqual(architecture.cflags, ("-march=rv64gc", "-mabi=lp64d"))

    def test_managed_toolchain_is_discovered_without_environment(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            build_root = Path(temporary)
            compiler = (build_root / "toolchains/rv/bin/"
                        "riscv64-buildroot-linux-musl-gcc")
            compiler.parent.mkdir(parents=True)
            compiler.write_text("managed", encoding="utf-8")
            with mock.patch.object(userland, "BUILD_ROOT", build_root), \
                    mock.patch.dict("os.environ", {}, clear=True):
                architecture = userland.load_architecture("rv")
            self.assertEqual(
                architecture.cross_compile,
                str(compiler.parent / "riscv64-buildroot-linux-musl-"),
            )

    def test_profiles_resolve_dependencies_once(self) -> None:
        profile = userland.load_profile("operator")
        packages = userland.resolve_packages(profile, "rv")
        self.assertEqual([package.name for package in packages],
                         ["base-layout", "busybox", "operator-tools"])
        self.assertIn("/var/lib/wateros", profile.overlay_replace_prefixes)

    def test_nanox_profile_is_riscv_only_and_dependency_ordered(self) -> None:
        profile = userland.load_profile("nanox")
        packages = userland.resolve_packages(profile, "rv")
        self.assertEqual([package.name for package in packages],
                         ["base-layout", "busybox", "operator-tools", "microwindows"])
        with self.assertRaisesRegex(userland.UserlandError, "does not support la"):
            userland.resolve_packages(profile, "la")

    def test_nanox_doom_payload_and_launcher_are_present(self) -> None:
        wad = (userland.USER_ROOT / "vendor/microwindows/src/contrib/doom/"
               "doom1.wad")
        self.assertTrue(wad.is_file())
        self.assertIn(wad.read_bytes()[:4], (b"IWAD", b"PWAD"))
        launcher = (userland.PACKAGE_ROOT / "microwindows/scripts/"
                    "start-doom")
        self.assertTrue(launcher.is_file())
        self.assertIn("DOOMWADDIR", launcher.read_text(encoding="utf-8"))

    def test_dependency_cycle_is_rejected(self) -> None:
        def package(name: str) -> userland.Package:
            dependency = "right" if name == "left" else "left"
            return userland.Package(name, "1", Path("."), None, ("rv",),
                                    (dependency,), Path(__file__), "/", (), ())

        profile = userland.Profile("cycle", ("left",), (), ())
        with mock.patch.object(userland, "load_package", side_effect=package):
            with self.assertRaisesRegex(userland.UserlandError, "dependency cycle"):
                userland.resolve_packages(profile, "rv")


class CompositionTests(unittest.TestCase):
    def package(self, name: str, overwrite: tuple[str, ...] = ()) -> userland.Package:
        return userland.Package(name, "1", Path("."), None, ("rv",), (),
                                Path(__file__), "/", overwrite, ())

    def profile(self) -> userland.Profile:
        return userland.Profile("test", (), (), ())

    def test_path_collision_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = root / "first"
            second = root / "second"
            staging = root / "staging"
            for source, content in ((first, "one"), (second, "two")):
                (source / "bin").mkdir(parents=True)
                (source / "bin/tool").write_text(content, encoding="utf-8")
            owners: dict[str, str] = {}
            userland.merge_package(first, staging, package=self.package("first"),
                                   profile=self.profile(), owners=owners)
            with self.assertRaisesRegex(userland.UserlandError, "already owned"):
                userland.merge_package(second, staging, package=self.package("second"),
                                       profile=self.profile(), owners=owners)

    def test_explicit_overwrite_is_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = root / "first"
            second = root / "second"
            staging = root / "staging"
            for source, content in ((first, "one"), (second, "two")):
                (source / "bin").mkdir(parents=True)
                (source / "bin/tool").write_text(content, encoding="utf-8")
            owners: dict[str, str] = {}
            userland.merge_package(first, staging, package=self.package("first"),
                                   profile=self.profile(), owners=owners)
            userland.merge_package(second, staging,
                                   package=self.package("second", ("/bin/tool",)),
                                   profile=self.profile(), owners=owners)
            self.assertEqual((staging / "bin/tool").read_text(encoding="utf-8"), "two")
            self.assertEqual(owners["/bin/tool"], "second")

    def test_cache_key_changes_with_input(self) -> None:
        architecture = userland.load_architecture("rv")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package_dir = root / "package"
            package_dir.mkdir()
            build_script = package_dir / "build.py"
            build_script.write_text("# first\n", encoding="utf-8")
            package = userland.Package("sample", "1", package_dir, None, ("rv",), (),
                                       build_script, "/", (), ())
            before = userland.package_cache_key(package, architecture, "cc 1")
            build_script.write_text("# second\n", encoding="utf-8")
            after = userland.package_cache_key(package, architecture, "cc 1")
            self.assertNotEqual(before, after)

    def test_cache_key_ignores_python_bytecode(self) -> None:
        architecture = userland.load_architecture("rv")
        with tempfile.TemporaryDirectory() as temporary:
            package_dir = Path(temporary) / "package"
            package_dir.mkdir()
            build_script = package_dir / "build.py"
            build_script.write_text("# source\n", encoding="utf-8")
            package = userland.Package("sample", "1", package_dir, None, ("rv",), (),
                                       build_script, "/", (), ())
            before = userland.package_cache_key(package, architecture, "cc 1")
            bytecode = package_dir / "__pycache__/build.cpython-312.pyc"
            bytecode.parent.mkdir()
            bytecode.write_bytes(b"host-specific bytecode")
            after = userland.package_cache_key(package, architecture, "cc 1")
            self.assertEqual(before, after)

    def test_manifest_is_path_sorted_and_hashes_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "z").write_bytes(b"z")
            (root / "a").mkdir()
            (root / "a/file").write_bytes(b"wateros")
            manifest = userland.file_manifest(root)
            paths = [entry["path"] for entry in manifest]
            self.assertEqual(paths, ["/a", "/z", "/a/file"])
            file_entry = next(entry for entry in manifest if entry["path"] == "/a/file")
            self.assertEqual(file_entry["sha256"], hashlib.sha256(b"wateros").hexdigest())


class PackageBuildTests(unittest.TestCase):
    def test_base_layout_permissions_and_mount_points(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "destdir"
            destination.mkdir()
            context = root / "context.json"
            context.write_text(json.dumps({
                "user_root": str(userland.USER_ROOT),
                "destdir": str(destination),
            }), encoding="utf-8")
            subprocess.run([sys.executable,
                            str(userland.PACKAGE_ROOT / "base-layout/build.py"),
                            "--context", str(context)], check=True)
            self.assertEqual((destination / "root").stat().st_mode & 0o7777, 0o700)
            for path in ("tmp", "var/tmp", "dev/shm"):
                self.assertEqual((destination / path).stat().st_mode & 0o7777, 0o1777)
            self.assertEqual((destination / "var/run").readlink(), Path("../run"))
            self.assertTrue((destination / "etc/profile.d").is_dir())


if __name__ == "__main__":
    unittest.main()
