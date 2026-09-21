#!/usr/bin/env bash
# Prepare a TFTP tree and run a foreground dnsmasq TFTP server for 2K1000.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OS_DIR="$(cd "$HERE/../.." && pwd)"
IMAGE=""
BOOT_DIR="$OS_DIR/build/la2k-boot"
TFTP_ROOT="$OS_DIR/build/la2k-tftp"
SERVER_IP="192.168.1.2"
BOARD_IP="192.168.1.20"
CHUNK_MB=32
LOAD_ADDR="0x9000000094000000"
START_SERVER=1

die() { printf 'error: %s\n' "$*" >&2; exit 2; }
info() { printf '[la2k-tftp] %s\n' "$*"; }
usage() {
    cat >&2 <<'EOF'
Usage: prepare_tftp.sh --image wateros-la.img [options]

Options:
  --boot-dir DIR       boot files (default: os/build/la2k-boot)
  --tftp-root DIR      TFTP directory (default: os/build/la2k-tftp)
  --server-ip ADDR     host address used by U-Boot (default: 192.168.1.2)
  --board-ip ADDR      board address shown in instructions (default: 192.168.1.20)
  --chunk-mb N         image chunk size (default: 32)
  --prepare-only       prepare files and print commands without starting dnsmasq
EOF
    exit "${1:-2}"
}

while (($#)); do
    case "$1" in
        --image) (($# >= 2)) || die "missing image path"; IMAGE="$2"; shift 2 ;;
        --boot-dir) (($# >= 2)) || die "missing boot directory"; BOOT_DIR="$2"; shift 2 ;;
        --tftp-root) (($# >= 2)) || die "missing TFTP root"; TFTP_ROOT="$2"; shift 2 ;;
        --server-ip) (($# >= 2)) || die "missing server IP"; SERVER_IP="$2"; shift 2 ;;
        --board-ip) (($# >= 2)) || die "missing board IP"; BOARD_IP="$2"; shift 2 ;;
        --chunk-mb) (($# >= 2)) || die "missing chunk size"; CHUNK_MB="$2"; shift 2 ;;
        --prepare-only) START_SERVER=0; shift ;;
        -h|--help) usage 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

[[ -n "$IMAGE" ]] || usage
[[ -f "$IMAGE" ]] || die "image not found: $IMAGE"
image_bytes=$(stat -c '%s' "$IMAGE")
((image_bytes % 512 == 0)) || die "image size must be a multiple of 512 bytes"
[[ "$CHUNK_MB" =~ ^[1-9][0-9]*$ ]] || die "--chunk-mb must be a positive integer"
for tool in split stat mkimage realpath; do
    command -v "$tool" >/dev/null 2>&1 || die "missing host command: $tool"
done
if ((START_SERVER)); then
    for tool in dnsmasq sudo ip id; do
        command -v "$tool" >/dev/null 2>&1 || die "missing host command: $tool"
    done
fi
[[ -d "$BOOT_DIR" ]] || die "boot directory not found: $BOOT_DIR"
TFTP_ROOT="$(realpath -m "$TFTP_ROOT")"
[[ "$TFTP_ROOT" != / ]] || die "refusing to use / as the TFTP root"
for file in kernel-la2k.bin loongson2k1000.dtb boot.scr; do
    [[ -f "$BOOT_DIR/$file" ]] || die "missing boot file: $BOOT_DIR/$file"
done
if ((START_SERVER)) &&
   ! ip -4 -o addr show | awk '{print $4}' | cut -d/ -f1 | grep -qx "$SERVER_IP"; then
    die "host has no IPv4 address $SERVER_IP; configure the Ethernet interface first"
fi

mkdir -p "$TFTP_ROOT"
rm -f "$TFTP_ROOT"/wateros-la.img.part.* "$TFTP_ROOT"/wateros-2k1000-flash.{cmd,scr}
split -b "${CHUNK_MB}M" -d -a 4 "$IMAGE" \
    "$TFTP_ROOT/wateros-la.img.part."
chunks=("$TFTP_ROOT"/wateros-la.img.part.*)
((${#chunks[@]} > 0)) || die "failed to split image"

cmdfile="$TFTP_ROOT/wateros-2k1000-flash.cmd"
{
    printf '%s\n' \
        "setenv serverip $SERVER_IP" \
        "setenv ipaddr $BOARD_IP" \
        "setenv chunk_addr $LOAD_ADDR" \
        "setenv fdt_high 0xffffffffffffffff" \
        "scsi reset" \
        "scsi dev 0"
    lba=0
    for chunk in "${chunks[@]}"; do
        name="$(basename "$chunk")"
        blocks=$(( $(stat -c '%s' "$chunk") / 512 ))
        printf 'if tftpboot ${chunk_addr} %s; then\n' "$name"
        printf '  setexpr chunk_blocks 0x%x\n' "$blocks"
        printf '  if scsi write ${chunk_addr} 0x%x ${chunk_blocks}; then echo Wrote %s; else echo SATA write failed; exit; fi\n' "$lba" "$name"
        printf 'else echo TFTP failed for %s; exit; fi\n' "$name"
        lba=$((lba + blocks))
    done
    printf '%s\n' \
        "setenv wateros_boot 'setenv scriptaddr $LOAD_ADDR; scsi reset; scsi dev 0; fatload scsi 0:1 \${scriptaddr} boot.scr; source \${scriptaddr}'" \
        "setenv bootcmd 'run wateros_boot'" \
        "setenv bootdelay 0" \
        "saveenv" \
        "echo WaterOS SATA image written" \
        "run wateros_boot"
} > "$cmdfile"
if ! mkimage -A loongarch -T script -C none -n "WaterOS 2K1000 chunked SATA flash" \
    -d "$cmdfile" "$TFTP_ROOT/wateros-2k1000-flash.scr" >/dev/null 2>&1; then
    # Older distro u-boot-tools releases do not know IH_ARCH_LOONGARCH.  U-Boot's
    # `source` path validates the script type and checksum, so retain their
    # architecture-default encoding as a compatibility fallback.
    mkimage -T script -C none -n "WaterOS 2K1000 chunked SATA flash" -d "$cmdfile" \
        "$TFTP_ROOT/wateros-2k1000-flash.scr" >/dev/null
fi

for file in kernel-la2k.bin loongson2k1000.dtb boot.scr; do
    cp -f "$BOOT_DIR/$file" "$TFTP_ROOT/$file"
done

info "TFTP root: $TFTP_ROOT"
info "image: $IMAGE ($image_bytes bytes, ${#chunks[@]} chunk(s), ${CHUNK_MB} MiB each)"
info "flash script: $TFTP_ROOT/wateros-2k1000-flash.scr"
cat <<EOF

在板子的 U-Boot 中执行：

setenv serverip $SERVER_IP
setenv ipaddr $BOARD_IP
tftpboot 0x9000000091000000 wateros-2k1000-flash.scr
source 0x9000000091000000

脚本会自动分片 TFTP、写入 SATA LBA 0、保存 bootcmd，并从 GPT P1 加载
boot.scr；该脚本再以 kernel-la2k.bin + loongson2k1000.dtb 启动内核。

EOF

if ((!START_SERVER)); then
    info "prepare-only requested; TFTP server was not started"
    exit 0
fi

info "dnsmasq will run in the foreground; press Ctrl-C to stop the TFTP server"

serve_user="$(id -un)"
serve_group="$(id -gn)"
exec sudo dnsmasq --no-daemon --port=0 --enable-tftp \
    --user="$serve_user" --group="$serve_group" \
    --tftp-root="$TFTP_ROOT" --listen-address="$SERVER_IP"
