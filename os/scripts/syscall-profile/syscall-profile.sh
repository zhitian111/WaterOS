#!/usr/bin/env bash
# 构建并运行 syscall-profile QEMU 插件。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WOS_LOG_COMPONENT=SYSCALL
source "$HERE/../source/console.bash"

usage() {
    cat <<'EOF'
用法:
  syscall-profile.sh <rv|la> build
  syscall-profile.sh <rv|la> run <output.txt> [plugin-option ...] -- <qemu command...>

参数:
  rv|la          Guest 架构
  build          只构建对应架构的 QEMU TCG plugin
  run            构建 plugin，并执行分隔符 -- 后的 QEMU 命令
  output.txt     原始画像输出文件
  plugin-option  追加到 plugin 参数的 key=value，可重复传入

常用 plugin-option:
  backend=auto paths=1 max_path=256 top_paths=200 user_max=0x80000000

示例:
  ./syscall-profile-rv.sh run /tmp/syscalls.txt backend=ecall -- \
      timeout 120 qemu-system-riscv64 -machine virt -kernel ../../kernel-rv-final ...
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

ARCH="${1:-}"
if [ $# -gt 0 ]; then shift; fi
case "$ARCH" in
    rv|riscv) ARCH=rv ;;
    la|loongarch64) ARCH=la ;;
    *) usage; exit 2 ;;
esac

COMMAND="${1:-}"
if [ $# -gt 0 ]; then shift; fi
if [[ "$COMMAND" == "-h" || "$COMMAND" == "--help" || "$COMMAND" == "help" ]]; then
    usage
    exit 0
fi
BUILD_DIR="$HERE/build/$ARCH"
SO="$BUILD_DIR/syscall-profile-$ARCH.so"

build() {
    local -a glib_cflags
    mkdir -p "$BUILD_DIR"
    read -r -a glib_cflags <<< "$(pkg-config --cflags glib-2.0)"
    gcc "${glib_cflags[@]}" -shared -fPIC -O2 -Wall -Wextra \
        -o "$SO" "$HERE/syscall-profile.c"
    info "系统调用分析插件已构建 path=${SO}"
}

run_qemu() {
    local output="${1:?必须提供输出路径}"
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
    info "开始采集系统调用 plugin=${SO} output=${output}"
    exec "$@" -plugin "$plugin"
}

case "$COMMAND" in
    build) build ;;
    run) run_qemu "$@" ;;
    *) usage; exit 2 ;;
esac
