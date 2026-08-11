#!/usr/bin/env bash
# Build and run the syscall-profile QEMU plugin.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat <<'EOF'
usage:
  syscall-profile.sh <rv|la> build
  syscall-profile.sh <rv|la> run <output.txt> [plugin-option ...] -- <qemu command...>

plugin options are appended after out=, for example:
  backend=auto paths=1 max_path=256 top_paths=200 user_max=0x80000000

example:
  ./syscall-profile-rv.sh run /tmp/syscalls.txt backend=ecall -- \
      timeout 120 qemu-system-riscv64 -machine virt -kernel ../../kernel-rv-final ...
EOF
}

ARCH="${1:-}"
if [ $# -gt 0 ]; then shift; fi
case "$ARCH" in
    rv|riscv) ARCH=rv ;;
    la|loongarch64) ARCH=la ;;
    *) usage; exit 2 ;;
esac

COMMAND="${1:-}"
if [ $# -gt 0 ]; then shift; fi
BUILD_DIR="$HERE/build/$ARCH"
SO="$BUILD_DIR/syscall-profile-$ARCH.so"

build() {
    local -a glib_cflags
    mkdir -p "$BUILD_DIR"
    read -r -a glib_cflags <<< "$(pkg-config --cflags glib-2.0)"
    gcc "${glib_cflags[@]}" -shared -fPIC -O2 -Wall -Wextra \
        -o "$SO" "$HERE/syscall-profile.c"
    echo "built: $SO"
}

run_qemu() {
    local output="${1:?output path required}"
    shift
    local plugin="file=$SO,out=$output"
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        plugin+=",$1"
        shift
    done
    if [ "${1:-}" = "--" ]; then shift; fi
    if [ $# -eq 0 ]; then
        usage
        exit 2
    fi
    if [ ! -f "$SO" ]; then
        build
    fi
    echo "[syscall-profile] plugin=$SO out=$output" >&2
    exec "$@" -plugin "$plugin"
}

case "$COMMAND" in
    build) build ;;
    run) run_qemu "$@" ;;
    *) usage; exit 2 ;;
esac

