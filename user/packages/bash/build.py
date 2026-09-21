#!/usr/bin/env python3
"""Build a statically linked GNU bash for the target architecture."""
import argparse, json, os, shutil, subprocess, urllib.request
from pathlib import Path

URL = "https://ftp.gnu.org/gnu/bash/bash-5.2.37.tar.gz"

def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--context", required=True, type=Path)
    context = json.loads(parser.parse_args().context.read_text())
    root, work = Path(context["user_root"]), Path(context["work_dir"])
    archive = root / "build/downloads/bash-5.2.37.tar.gz"; archive.parent.mkdir(parents=True, exist_ok=True)
    if not archive.is_file(): urllib.request.urlretrieve(URL, archive)
    subprocess.run(["tar", "-xzf", str(archive), "--strip-components=1", "-C", str(work)], check=True)
    cross = context["cross_compile"]; env = os.environ.copy(); env.update(CC=cross+"gcc", AR=cross+"ar", RANLIB=cross+"ranlib", CFLAGS=" ".join(context["cflags"] + ["-O2", "-std=gnu89"]), LDFLAGS="-static")
    prefix = Path(context["destdir"]) / "usr"; prefix.mkdir(parents=True, exist_ok=True)
    subprocess.run(["./configure", "--host="+context["triple"], "--prefix=/usr", "--disable-nls", "--without-bash-malloc", "--disable-readline", "--enable-static-link"], cwd=work, env=env, check=True)
    # Bash generates a few host-side tools written in K&R C.  GCC 15 defaults
    # reject their empty parameter-list declarations, while gnu89 remains the
    # language mode Bash itself expects for these build-only helpers.
    subprocess.run(["make", "-j" + str(context["jobs"]),
                    "CC_FOR_BUILD=gcc -std=gnu89"],
                   cwd=work, env=env, check=True)
    subprocess.run(["make", "install", "DESTDIR="+str(context["destdir"])], cwd=work, env=env, check=True)
    binary = Path(context["destdir"]) / "usr/bin/bash"; binary.chmod(0o755)
    link = Path(context["destdir"]) / "bin/bash"; link.parent.mkdir(exist_ok=True); link.symlink_to("../usr/bin/bash")
    return 0
if __name__ == "__main__": raise SystemExit(main())
