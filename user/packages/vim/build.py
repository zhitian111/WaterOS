#!/usr/bin/env python3
"""Build Vim's tiny, terminal-only editor with a private static ncurses."""
import argparse, json, os, subprocess, urllib.request
from pathlib import Path

VIM_URL = "https://github.com/vim/vim/archive/refs/tags/v9.1.1590.tar.gz"
NC_URL = "https://invisible-island.net/archives/ncurses/ncurses.tar.gz"

def fetch(url: str, path: Path) -> None:
    if not path.is_file(): urllib.request.urlretrieve(url, path)

def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--context", required=True, type=Path)
    context = json.loads(parser.parse_args().context.read_text()); root, work = Path(context["user_root"]), Path(context["work_dir"])
    downloads = root / "build/downloads/vim"; downloads.mkdir(parents=True, exist_ok=True)
    fetch(VIM_URL, downloads / "vim.tar.gz"); fetch(NC_URL, downloads / "ncurses.tar.gz")
    (work / "vim-src").mkdir(); (work / "ncurses-src").mkdir()
    subprocess.run(["tar", "-xzf", str(downloads/"vim.tar.gz"), "--strip-components=1", "-C", str(work/"vim-src")], check=True)
    subprocess.run(["tar", "-xzf", str(downloads/"ncurses.tar.gz"), "--strip-components=1", "-C", str(work/"ncurses-src")], check=True)
    cross=context["cross_compile"]; env=os.environ.copy(); env.update(CC=cross+"gcc", AR=cross+"ar", RANLIB=cross+"ranlib", CFLAGS=" ".join(context["cflags"]+["-O2"]), LDFLAGS="-static")
    prefix=work/"prefix"; subprocess.run(["./configure", "--host="+context["triple"], "--prefix="+str(prefix), "--without-shared", "--without-debug", "--without-ada", "--enable-widec", "--without-cxx-binding"], cwd=work/"ncurses-src", env=env, check=True); subprocess.run(["make", "-j"+str(context["jobs"])], cwd=work/"ncurses-src", env=env, check=True); subprocess.run(["make", "install"], cwd=work/"ncurses-src", env=env, check=True)
    vim_env=env.copy(); vim_env["CPPFLAGS"]="-I"+str(prefix/"include"); vim_env["LDFLAGS"]="-static -L"+str(prefix/"lib")
    dest=Path(context["destdir"]); subprocess.run(["./configure", "--host="+context["triple"], "--prefix=/usr", "--with-features=tiny", "--enable-gui=no", "--without-x", "--disable-nls", "--disable-cscope", "--with-tlib=ncursesw"], cwd=work/"vim-src", env=vim_env, check=True); subprocess.run(["make", "-j"+str(context["jobs"])], cwd=work/"vim-src", env=vim_env, check=True); subprocess.run(["make", "install", "DESTDIR="+str(dest)], cwd=work/"vim-src", env=vim_env, check=True)
    return 0
if __name__ == "__main__": raise SystemExit(main())
