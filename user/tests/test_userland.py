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

    def test_packages_resolve_dependencies_once(self) -> None:
        packages = userland.resolve_packages(("operator-tools",), "rv")
        self.assertEqual([package.name for package in packages],
                         ["base-layout", "busybox", "operator-tools"])
        self.assertIn("/var/lib/wateros", userland.OVERLAY_REPLACE_PREFIXES)

    def test_graphics_package_is_supported_and_dependency_ordered_on_both_arches(self) -> None:
        packages = userland.resolve_packages(("microwindows",), "rv")
        self.assertEqual([package.name for package in packages],
                         ["base-layout", "busybox", "operator-tools", "microwindows"])
        la_packages = userland.resolve_packages(("microwindows",), "la")
        self.assertEqual([package.name for package in la_packages],
                         ["base-layout", "busybox", "operator-tools", "microwindows"])

    def test_all_selects_every_package_supported_by_architecture(self) -> None:
        rv = userland.parse_package_names("all", "rv")
        la = userland.parse_package_names("all", "la")
        self.assertIn("microwindows", rv)
        self.assertIn("microwindows", la)
        self.assertIn("openjdk21", rv)
        self.assertIn("openjdk21", la)
        self.assertEqual(userland.parse_package_names("busybox, operator-tools", "rv"),
                         ("busybox", "operator-tools"))

    def test_exclusion_cascades_to_dependent_packages(self) -> None:
        selected = userland.parse_package_names("all", "rv")
        kept, skipped = userland.exclude_packages(selected, ("mgba",), "rv")
        self.assertNotIn("mgba", kept)
        self.assertNotIn("waterfm", kept)
        self.assertEqual(skipped["mgba"], ("mgba",))
        self.assertEqual(skipped["waterfm"], ("mgba",))
        self.assertIn("microwindows", kept)

    def test_empty_exclusion_keeps_selection(self) -> None:
        selected = ("base-layout", "busybox")
        self.assertEqual(userland.exclude_packages(selected, (), "rv"),
                         (selected, {}))

    def test_openjdk21_is_supported_and_dependency_ordered_on_both_arches(self) -> None:
        packages = userland.resolve_packages(("openjdk21",), "rv")
        self.assertEqual([package.name for package in packages],
                         ["base-layout", "busybox", "openjdk21"])
        la_packages = userland.resolve_packages(("openjdk21",), "la")
        self.assertEqual([package.name for package in la_packages],
                         ["base-layout", "busybox", "openjdk21"])

        package = userland.PACKAGE_ROOT / "openjdk21"
        build = (package / "build.py").read_text(encoding="utf-8")
        self.assertIn("openjdk21-jre-headless-21.0.11_p10-r0.apk", build)
        self.assertIn("41e1a3a1234c6cf5014d46288ed0d3c0b475e162d1384e42018eb13fbf47726c",
                      build)
        self.assertIn("ca-certificates-bundle-20260611-r0.apk", build)
        self.assertIn("loongson21.11.38-fx-jdk21.0.11_10-linux-loongarch64-glibc2.34.tar.gz",
                      build)
        self.assertIn("3fd0b56e2e060d668f5392c9fb9f2c6f243b4fcc6daaea01dabfdea6e5d153fb",
                      build)
        self.assertIn("zlib-1.3.2.tar.gz", build)
        self.assertIn("bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16",
                      build)
        self.assertIn("d8688143c6107456a13d959ae23c9f375ee2b743c8bb5f59be77d9b5ac956173",
                      build)
        self.assertIn("94034130a5be3970f06739c6653922b77e4652d82af1f660752d416236e51c28",
                      build)
        self.assertTrue((package / "scripts/wos-jvm-smoke").is_file())
        smoke = (package / "scripts/wos-jvm-smoke").read_text(encoding="utf-8")
        self.assertIn("-XX:-UseCompressedOops", smoke)
        self.assertIn("-XX:-UseCompressedClassPointers", smoke)
        self.assertTrue((package / "scripts/wos-jvm-network").is_file())
        self.assertTrue((package / "scripts/wos-jvm-application").is_file())
        self.assertTrue((package / "scripts/wos-jvm-strict").is_file())
        self.assertTrue((package / "tests/RuntimeProbe.class.b64").is_file())
        self.assertTrue((package / "tests/NetworkProbe.class.b64").is_file())
        self.assertTrue((package / "tests/ExceptionProbe.class.b64").is_file())
        self.assertTrue((package / "tests/JitProbe.class.b64").is_file())
        self.assertTrue((package / "tests/ApplicationProbe.jar.b64").is_file())
        self.assertTrue((package / "assets/cacerts").is_file())

    def test_nanox_doom_payload_and_launcher_are_present(self) -> None:
        wad = (userland.USER_ROOT / "vendor/microwindows/src/contrib/doom/"
               "doom1.wad")
        self.assertTrue(wad.is_file())
        self.assertIn(wad.read_bytes()[:4], (b"IWAD", b"PWAD"))
        launcher = (userland.PACKAGE_ROOT / "microwindows/scripts/"
                    "start-doom")
        self.assertTrue(launcher.is_file())
        self.assertIn("DOOMWADDIR", launcher.read_text(encoding="utf-8"))
        menu = (userland.PACKAGE_ROOT / "microwindows/config/"
                "nxlaunch.cnf").read_text(encoding="utf-8")
        self.assertIn("Doom /usr/share/wateros/icons/doom.ppm "
                      "/bin/sh /usr/bin/start-doom", menu)
        self.assertNotIn("Doom - /usr/bin/start-doom", menu)

    def test_nanox_desktop_assets_are_reproducible_ppm(self) -> None:
        package = userland.PACKAGE_ROOT / "microwindows"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            subprocess.run([
                sys.executable, str(package / "tools/prepare_assets.py"),
                "--source", str(package / "assets/wateros-waves.png"),
                "--output", str(output),
            ], check=True)
            wallpaper = output / "wallpapers/wateros-waves.ppm"
            self.assertTrue(wallpaper.read_bytes().startswith(b"P6\n1280 800\n255\n"))
            for name in ("terminal", "files", "editor", "calculator",
                         "clock", "doom", "mgba"):
                icon = output / f"icons/{name}.ppm"
                self.assertTrue(icon.read_bytes().startswith(b"P6\n40 40\n255\n"))

        menu = (package / "config/nxlaunch.cnf").read_text(encoding="utf-8")
        self.assertIn("$window_background_mode 4", menu)
        self.assertIn("wateros-waves.ppm", menu)
        launcher = (package / "scripts/start-nanox").read_text(encoding="utf-8")
        self.assertNotIn("nxeyes &", launcher)

    def test_nanox_input_and_present_optimizations_are_configured(self) -> None:
        package = userland.PACKAGE_ROOT / "microwindows"
        optimization = (package / "patches/"
                        "0005-wateros-input-present-doom-performance.patch")
        text = optimization.read_text(encoding="utf-8")
        all_patches = "\n".join(
            patch.read_text(encoding="utf-8")
            for patch in sorted((package / "patches").glob("*.patch")))
        qwerty = "qwertyuiopasdfghjklzxcvbnm"
        for letter in qwerty:
            mapping = f"[KEY_{letter.upper()}] = '{letter}'"
            self.assertIn(mapping, text)
        added_lines = "\n".join(
            line[1:] for line in text.splitlines()
            if line.startswith("+") and not line.startswith("+++"))
        self.assertNotIn("code - KEY_A", added_lines)
        self.assertIn("WOSFBIO_FLUSH_RECT", text)
        self.assertIn("_IOW('W', 0x01, struct wos_fb_rect)", text)
        self.assertIn("upper = shift ^ caps_lock", text)
        self.assertIn("code == KEY_CAPSLOCK && value == 1", text)
        self.assertIn("event.value == 0 ? KBD_KEYRELEASE : KBD_KEYPRESS", all_patches)
        self.assertIn("SYN_DROPPED", text)
        self.assertIn("GrReqShmCmds(3 * 1024 * 1024)", text)
        self.assertIn("render_scaled_frame", text)
        config = (package / "config/wateros").read_text(encoding="utf-8")
        self.assertIn("HAVE_SHAREDMEM_SUPPORT   = Y", config)
        self.assertFalse((package / "patches/0003-wateros-present-every-loop.patch").exists())
        launcher = (package / "scripts/start-doom").read_text(encoding="utf-8")
        self.assertIn('set -- -2 "$@"', launcher)

    def test_dependency_cycle_is_rejected(self) -> None:
        def package(name: str) -> userland.Package:
            dependency = "right" if name == "left" else "left"
            return userland.Package(name, "1", Path("."), None, ("rv",),
                                    (dependency,), Path(__file__), "/", (), ())

        with mock.patch.object(userland, "load_package", side_effect=package):
            with self.assertRaisesRegex(userland.UserlandError, "dependency cycle"):
                userland.resolve_packages(("left",), "rv")


class CompositionTests(unittest.TestCase):
    def package(self, name: str, overwrite: tuple[str, ...] = ()) -> userland.Package:
        return userland.Package(name, "1", Path("."), None, ("rv",), (),
                                Path(__file__), "/", overwrite, ())

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
                                   owners=owners)
            with self.assertRaisesRegex(userland.UserlandError, "already owned"):
                userland.merge_package(second, staging, package=self.package("second"),
                                       owners=owners)

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
                                   owners=owners)
            userland.merge_package(second, staging,
                                   package=self.package("second", ("/bin/tool",)),
                                   owners=owners)
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
