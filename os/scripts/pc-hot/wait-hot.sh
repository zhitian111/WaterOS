#!/usr/bin/env bash
# wait-hot：构建并运行 QEMU plugin，在不修改内核的情况下记录各 vCPU 的
# idle 时间和阻塞系统调用墙钟时间。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"

usage() {
    cat <<'EOF'
usage:
  wait-hot.sh <rv|la> build
  wait-hot.sh <rv|la> run <out.txt> -- <qemu args...>
EOF
}

ARCH="${1:-}"
if [ $# -gt 0 ]; then shift; fi
case "$ARCH" in
    rv|riscv) ARCH=rv ;;
    la|loongarch64) ARCH=la ;;
    *) usage; exit 2 ;;
esac

CMD="${1:-}"
if [ $# -gt 0 ]; then shift; fi

BUILD_DIR="$HERE/build/$ARCH"
SO="$BUILD_DIR/wait-hot-$ARCH.so"
mkdir -p "$BUILD_DIR"

build() {
    local -a glib_cflags
    read -r -a glib_cflags <<< "$(pkg-config --cflags glib-2.0)"
    gcc "${glib_cflags[@]}" -shared -fPIC -O2 -o "$SO" "$HERE/wait-hot.c"
    echo "built: $SO"
}

run_qemu() {
    local out="${1:?out path required}"
    shift
    if [ "${1:-}" = "--" ]; then shift; fi
    build
    exec "$@" -plugin "file=$SO,out=$out"
}

case "$CMD" in
    build) build ;;
    run) run_qemu "$@" ;;
    *) usage; exit 2 ;;
esac
