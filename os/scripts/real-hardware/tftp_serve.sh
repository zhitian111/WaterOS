#!/usr/bin/env bash
# 启动 WaterOS 2K1000 真机 TFTP 服务（dnsmasq 前台运行，Ctrl-C 停止）。
#
# 用法：
#   tftp_serve.sh [listen_ip] [tftp_root] [disk_image]
#
# 默认：
#   listen_ip = 192.168.1.2
#   tftp_root = /srv/tftp
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OS_DIR="$(cd "$HERE/../.." && pwd)"
WOS_LOG_COMPONENT=TFTP-SERVE
source "$HERE/../source/console.bash"

LISTEN_IP="${1:-192.168.1.2}"
TFTP_ROOT="${2:-/srv/tftp}"
DISK_IMAGE="${3:-$OS_DIR/../user/build/images/wateros-la.img}"

die() {
    error "$*" 1
    exit 1
}

for cmd in dnsmasq sudo stat; do
    command -v "$cmd" >/dev/null 2>&1 || die "缺少命令: $cmd"
done

sudo -v || die "sudo 权限不可用"

if ! ip -4 -o addr show | awk '{print $4}' | cut -d/ -f1 | grep -qx "$LISTEN_IP"; then
    warning "本机没有检测到 IPv4 地址 $LISTEN_IP"
    info "当前 IPv4 地址："
    ip -4 -o addr show | awk '{print "  "$2"  "$4}'
    die "请先配置网卡地址，例如: sudo ip addr add $LISTEN_IP/24 dev <网卡>"
fi

sudo mkdir -p "$TFTP_ROOT"

sync_file() {
    local src="$1"
    local dst="$2"
    [ -f "$src" ] || die "找不到 $src，请先在 os/ 目录运行 make la2k_uimage 和 make la2k_bootscr"
    sudo cp -f "$src" "$dst"
    sudo chmod 0644 "$dst"
    info "已同步: $src -> $dst"
}

sync_file "$OS_DIR/kernel-la2k.ui" "$TFTP_ROOT/kernel-la2k.ui"
sync_file "$OS_DIR/build/wateros-2k1000.scr" "$TFTP_ROOT/wateros-2k1000.scr"
sync_file "$OS_DIR/build/wateros-2k1000-flash.scr" "$TFTP_ROOT/wateros-2k1000-flash.scr"
sync_file "$DISK_IMAGE" "$TFTP_ROOT/wateros-la.img"

info "================ TFTP 服务 ================"
info "监听地址: $LISTEN_IP"
info "TFTP 根目录: $TFTP_ROOT"
info "内核: $(ls -l "$TFTP_ROOT/kernel-la2k.ui" | awk '{print $5}') bytes"
info "启动脚本: $(ls -l "$TFTP_ROOT/wateros-2k1000.scr" | awk '{print $5}') bytes"
info "烧录脚本: $(ls -l "$TFTP_ROOT/wateros-2k1000-flash.scr" | awk '{print $5}') bytes"
info "SATA 镜像: $(ls -l "$TFTP_ROOT/wateros-la.img" | awk '{print $5}') bytes"
info "停止方法: Ctrl-C"
info "=========================================="

exec sudo dnsmasq \
    --no-daemon \
    --port=0 \
    --enable-tftp \
    --tftp-root="$TFTP_ROOT" \
    --listen-address="$LISTEN_IP"
