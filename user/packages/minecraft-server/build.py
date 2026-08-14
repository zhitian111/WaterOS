#!/usr/bin/env python3
"""Install the pinned, architecture-independent Minecraft Java server."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import urllib.error
import urllib.request
from pathlib import Path


VERSION = "1.21.11"
SERVER_OBJECT = "64bb6d763bed0a9f1d632ec347938594144943ed"
SERVER_URL = f"https://piston-data.mojang.com/v1/objects/{SERVER_OBJECT}/server.jar"
SERVER_SHA1 = SERVER_OBJECT
DOWNLOAD_ACCEPTANCE = "MINECRAFT_EULA_DOWNLOAD_ACCEPTED"


def sha1_file(path: Path) -> str:
    digest = hashlib.sha1(usedforsecurity=False)
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def ensure_server_jar(destination: Path) -> None:
    if destination.is_file() and sha1_file(destination) == SERVER_SHA1:
        print(f"[minecraft-server] cache hit: {destination.name}", flush=True)
        return
    destination.unlink(missing_ok=True)
    if os.environ.get(DOWNLOAD_ACCEPTANCE) != "true":
        raise RuntimeError(
            f"Minecraft server {VERSION} is not cached. Downloading it means "
            "accepting the Minecraft EULA and Privacy Policy. Review "
            "https://www.minecraft.net/en-us/download/server and rerun with "
            f"{DOWNLOAD_ACCEPTANCE}=true, or place the official file at "
            f"{destination}. Expected SHA-1: {SERVER_SHA1}"
        )

    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(".jar.part")
    temporary.unlink(missing_ok=True)
    print(f"[minecraft-server] download: {SERVER_URL}", flush=True)
    request = urllib.request.Request(
        SERVER_URL, headers={"User-Agent": "WaterOS-userland/1"}
    )
    try:
        with urllib.request.urlopen(request) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output, 1024 * 1024)
        actual = sha1_file(temporary)
        if actual != SERVER_SHA1:
            raise RuntimeError(
                f"checksum mismatch for Minecraft server {VERSION}: "
                f"expected {SERVER_SHA1}, got {actual}"
            )
        os.replace(temporary, destination)
    except (OSError, urllib.error.URLError):
        temporary.unlink(missing_ok=True)
        raise


def install_script(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    destination.chmod(0o755)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    package = Path(context["package_dir"])
    root = Path(context["destdir"])

    cache = (Path(context["user_root"]) / "build/downloads/minecraft-server"
             / f"minecraft-server-{VERSION}.jar")
    ensure_server_jar(cache)

    server = root / "opt/minecraft/server.jar"
    server.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(cache, server)
    server.chmod(0o644)

    data = root / "var/lib/minecraft"
    data.mkdir(parents=True, exist_ok=True)
    data.chmod(0o755)

    install_script(package / "scripts/minecraft-server",
                   root / "usr/bin/minecraft-server")
    install_script(package / "scripts/wos-minecraft-smoke",
                   root / "opt/wateros/bin/wos-minecraft-smoke")
    install_script(package / "scripts/wos-minecraft-preflight",
                   root / "opt/wateros/bin/wos-minecraft-preflight")
    install_script(package / "scripts/wos-minecraft-jit-diagnostic",
                   root / "opt/wateros/bin/wos-minecraft-jit-diagnostic")
    install_script(package / "scripts/wos-minecraft-vm-info",
                   root / "opt/wateros/bin/wos-minecraft-vm-info")
    # Automated kernel runs use the stable /opt paths, while an interactive
    # BusyBox shell only places /usr/bin on PATH. Expose both entry points.
    for command in ("wos-minecraft-preflight", "wos-minecraft-smoke",
                    "wos-minecraft-jit-diagnostic", "wos-minecraft-vm-info"):
        link = root / "usr/bin" / command
        link.symlink_to(f"/opt/wateros/bin/{command}")

    documentation = root / "usr/share/doc/minecraft-server"
    documentation.mkdir(parents=True, exist_ok=True)
    shutil.copy2(package / "README-WaterOS.txt",
                 documentation / "README-WaterOS.txt")

    if sha1_file(server) != SERVER_SHA1:
        raise RuntimeError("installed Minecraft server failed checksum validation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
