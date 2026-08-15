#!/usr/bin/env bash
# 烧录 WaterOS 整盘镜像到指定块设备（带 y 确认与防呆校验）。
#
# 用法：
#   dd_image.sh <image> <device>
#   make dd_img_vf2 DEVICE=/dev/sdX      # VisionFive 2
#   make dd_img_2k1000 DEVICE=/dev/sdX   # Loongson 2K1000
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WOS_LOG_COMPONENT=DD-IMAGE
source "$HERE/../source/console.bash"

die() {
    error "$*" 1
    exit 1
}

[ "$#" -eq 2 ] || die "用法: $0 <image> <device>"

IMAGE="$1"
DEVICE="$2"

case "$DEVICE" in
    /dev/*) ;;
    *) die "设备必须是 /dev/ 下的绝对路径，当前: $DEVICE" ;;
esac

[ -f "$IMAGE" ] || die "镜像不存在: $IMAGE"
[ -b "$DEVICE" ] || die "不是块设备: $DEVICE（请先 lsblk 认盘）"

dev_type="$(lsblk -ndo TYPE "$DEVICE" 2>/dev/null || true)"
case "$dev_type" in
    disk|loop) ;;
    *) die "拒绝烧录: $DEVICE 不是整盘（type=$dev_type），of= 必须指向整盘而非分区" ;;
esac

# 防呆：拒绝系统盘（根文件系统所在盘）。
root_src="$(findmnt -n -o SOURCE / 2>/dev/null || true)"
root_disk="$(lsblk -no PKNAME "$root_src" 2>/dev/null || true)"
if [ -n "$root_disk" ] && [ "$DEVICE" = "/dev/$root_disk" ]; then
    die "拒绝烧录: $DEVICE 是系统盘"
fi

# 防呆：拒绝已挂载的盘。
if findmnt -rn -o SOURCE | grep -qx "$DEVICE" || lsblk -no MOUNTPOINTS "$DEVICE" | grep -q .; then
    die "拒绝烧录: $DEVICE 或其分区正在被挂载"
fi

img_bytes="$(stat -c %s "$IMAGE")"
dev_bytes="$(lsblk -bndo SIZE "$DEVICE" 2>/dev/null || echo 0)"
if [ "$dev_bytes" -lt "$img_bytes" ]; then
    die "拒绝烧录: 目标设备容量不足（镜像 $img_bytes B > 设备 $dev_bytes B）"
fi

info "================ 即将烧录 ================"
info "$(lsblk -o NAME,SIZE,TYPE,TRAN,MODEL "$DEVICE")"
info "镜像: $IMAGE ($(numfmt --to=iec "$img_bytes"))"
info "目标: $DEVICE ($(numfmt --to=iec "$dev_bytes"))"
info "=========================================="
read -r -p "输入 y 确认覆盖 $DEVICE 并开始烧录: " answer
[ "$answer" = "y" ] || die "已取消，未烧录"

sudo dd if="$IMAGE" of="$DEVICE" bs=4M conv=fsync oflag=direct status=progress
sudo sync
info "烧录完成: $IMAGE -> $DEVICE"
info "建议: 拔出重插后 lsblk/fdisk 确认分区表，再上板。"
