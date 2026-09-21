#!/usr/bin/env python3
"""Build Git with HTTPS clone support and no optional scripting runtimes."""
import argparse
import json
import os
import subprocess
import tarfile
import urllib.request
from pathlib import Path

GIT_URL = "https://github.com/git/git/archive/refs/tags/v2.49.0.tar.gz"
ZLIB_URL = "https://zlib.net/fossils/zlib-1.3.1.tar.gz"
OPENSSL_URL = "https://www.openssl.org/source/openssl-3.3.2.tar.gz"
CURL_URL = "https://curl.se/download/curl-8.10.1.tar.xz"
CA_BUNDLE_URL = ("https://dl-cdn.alpinelinux.org/alpine/v3.22/main/"
                 "riscv64/ca-certificates-bundle-20260611-r0.apk")

def fetch(url: str, path: Path) -> None:
    if not path.is_file(): urllib.request.urlretrieve(url, path)

def run(command: list[str], cwd: Path, env: dict[str, str]) -> None:
    print("[git]", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def install_ca_bundle(archive: Path, destination: Path) -> None:
    """Install the public CA bundle needed by HTTPS remotes."""
    with tarfile.open(archive, "r:gz") as bundle:
        source = bundle.extractfile("etc/ssl/certs/ca-certificates.crt")
        if source is None:
            raise RuntimeError("CA bundle archive lacks ca-certificates.crt")
        target = destination / "etc/ssl/certs/ca-certificates.crt"
        target.parent.mkdir(parents=True, exist_ok=True)
        with source, target.open("wb") as output:
            output.write(source.read())

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    c = json.loads(parser.parse_args().context.read_text())
    root, work = Path(c["user_root"]), Path(c["work_dir"])
    downloads = root / "build/downloads/git"
    downloads.mkdir(parents=True, exist_ok=True)
    archives = ((GIT_URL, "git.tar.gz"), (ZLIB_URL, "zlib.tar.gz"),
                (OPENSSL_URL, "openssl.tar.gz"), (CURL_URL, "curl.tar.xz"),
                (CA_BUNDLE_URL, "ca-certificates-bundle.apk"))
    for url, name in archives:
        fetch(url, downloads / name)
    for archive, name in (("git.tar.gz", "git"), ("zlib.tar.gz", "zlib"),
                          ("openssl.tar.gz", "openssl"), ("curl.tar.xz", "curl")):
        target = work / name
        target.mkdir()
        run(["tar", "-xf", str(downloads / archive), "--strip-components=1",
             "-C", str(target)], work, os.environ.copy())
    cross = c["cross_compile"]
    env = os.environ.copy()
    env.update(CC=cross + "gcc", AR=cross + "ar", RANLIB=cross + "ranlib",
               CFLAGS=" ".join(c["cflags"] + ["-O2"]), CPPFLAGS="", LDFLAGS="-static")
    prefix = work / "prefix"
    prefix.mkdir()
    run(["./configure", "--static", "--prefix=" + str(prefix)], work / "zlib", env)
    run(["make", "-j" + str(c["jobs"])], work / "zlib", env)
    run(["make", "install"], work / "zlib", env)
    ssl_target = "linux64-riscv64" if c["arch"] == "rv" else "linux-loongarch64"
    run(["./Configure", ssl_target, "no-shared", "no-tests", "no-apps",
         "--prefix=" + str(prefix)], work / "openssl", env)
    run(["make", "-j" + str(c["jobs"])], work / "openssl", env)
    run(["make", "install_sw"], work / "openssl", env)
    curl_env = env.copy()
    curl_env["CPPFLAGS"] = "-I" + str(prefix / "include")
    curl_env["LDFLAGS"] = "-static -L" + str(prefix / "lib")
    run(["./configure", "--host=" + c["triple"], "--prefix=" + str(prefix),
         "--disable-shared", "--enable-static", "--with-openssl=" + str(prefix),
         "--with-zlib=" + str(prefix), "--without-libpsl", "--without-brotli",
         "--without-zstd", "--disable-ldap", "--disable-rtsp", "--disable-dict",
         "--disable-telnet", "--disable-tftp", "--disable-pop3", "--disable-imap",
         "--disable-smtp", "--disable-gopher", "--disable-mqtt", "--disable-manual"],
        work / "curl", curl_env)
    run(["make", "-j" + str(c["jobs"])], work / "curl", curl_env)
    run(["make", "install"], work / "curl", curl_env)
    git_env = curl_env.copy()
    git_env.update(NO_GETTEXT="YesPlease", NO_PERL="YesPlease", NO_PYTHON="YesPlease",
                   NO_TCLTK="YesPlease", NO_INSTALL_HARDLINKS="YesPlease", CURL_CONFIG="",
                   CURL_LIBCURL="-lcurl -lssl -lcrypto -lz",
                   CURL_CFLAGS="-I" + str(prefix / "include"))
    build_args = ["prefix=/usr", "NO_INSTALL_HARDLINKS=YesPlease", "NO_GETTEXT=YesPlease",
                  "NO_PERL=YesPlease", "NO_PYTHON=YesPlease", "NO_TCLTK=YesPlease",
                  "NO_EXPAT=YesPlease", "LDFLAGS=-static -L" + str(prefix / "lib"),
                  "CURL_LIBCURL=-lcurl -lssl -lcrypto -lz",
                  "CURL_CFLAGS=-I" + str(prefix / "include")]
    run(["make", "-j" + str(c["jobs"]), *build_args], work / "git", git_env)
    run(["make", "install", "DESTDIR=" + str(Path(c["destdir"])), *build_args],
        work / "git", git_env)
    install_ca_bundle(downloads / "ca-certificates-bundle.apk", Path(c["destdir"]))
    return 0
if __name__ == "__main__": raise SystemExit(main())
