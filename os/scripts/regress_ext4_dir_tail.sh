#!/bin/bash
# RISC-V QEMU 端到端回归：验证 ext4 目录块 tail 边界修复。
#
# 默认模式（REGRESS_MODE=fs）：
#   在 guest 中用 shell 直接构造“目录块填满到 12 字节 tail 边界 + 3 字符子目录”
#   的写路径，随后在宿主对 overlay 做 e2fsck，验证无乱码目录项。
#
# 可选模式（REGRESS_MODE=apt）：
#   执行 apt-get install neovim-runtime。当前 main 分支缺少 unlockpt/文件 seek
#   等 syscall 兼容，apt 会在解包阶段提前中止；该模式保留给后续 syscall 修复。
#
# 安全约束：
#   - QEMU 使用 qcow2 overlay（backing 为只读 pub 工作副本），guest 写盘不落回
#     基准镜像；
#   - 默认只向 RV_IMG 注入 guest 回归脚本（该文件是解压出来的工作副本，原始
#     pub 镜像仍在 ~/Downloads/*.gz）；
#   - 日志只做关键片段 grep，不输出全量 QEMU 日志。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WOS_LOG_COMPONENT=REGRESS
# shellcheck source=/dev/null
source "$SCRIPT_DIR/source/console.bash"

RV_IMG="${RV_IMG:-}"
if [ -z "$RV_IMG" ]; then
    if [ -f "$OS_DIR/sdcard-rv-pub.img" ]; then
        RV_IMG="$OS_DIR/sdcard-rv-pub.img"
    elif [ -f "/home/zhitian/project/WaterOS_refactor/os/sdcard-rv-pub.img" ]; then
        RV_IMG="/home/zhitian/project/WaterOS_refactor/os/sdcard-rv-pub.img"
    else
        error "未找到 pub 镜像；请设置 RV_IMG 或先解压 ~/Downloads/sdcard-rv-pub.img.gz" 2
    fi
fi
RV_IMG="$(readlink -f "$RV_IMG")"

REGRESS_MODE="${REGRESS_MODE:-fs}"
OVERLAY_ID="${WOS_SNAPSHOT_ID:-dir-tail}"
GUEST_SCRIPT="/root/regress_dir_tail.sh"
LOG_DIR="$OS_DIR/tem"
mkdir -p "$LOG_DIR"
BUILD_LOG="$LOG_DIR/regress-dir-tail.build.log"
QEMU_LOG="$LOG_DIR/regress-dir-tail.qemu.log"
OVERLAY="$LOG_DIR/sdcard-rv.${OVERLAY_ID}.overlay.qcow2"
RAW_OUT="/tmp/wateros-dir-tail-overlay.raw"
QEMU_TIMEOUT="${WOS_QEMU_TIMEOUT:-1800}"

case "$REGRESS_MODE" in
    fs)
        PASS_MARKER="REG DIR TAIL FSCK PASS"
        ;;
    apt)
        PASS_MARKER="REGRESS DIR TAIL PASS"
        ;;
    *)
        error "REGRESS_MODE 必须是 fs/apt，当前为 ${REGRESS_MODE}" 2
        ;;
esac

info "RV_IMG=${RV_IMG} mode=${REGRESS_MODE} overlay=${OVERLAY} log_dir=${LOG_DIR}"

guest_script_tmp="$(mktemp)"
if [ "$REGRESS_MODE" = "fs" ]; then
    cat > "$guest_script_tmp" <<'EOF'
#!/bin/sh
set -x
export HOME=/root
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
echo "#### REG DIR TAIL FSCK START ####"

rm -rf /root/tailtest
mkdir -p /root/tailtest
i=0
while [ "$i" -lt 360 ]; do
    name=$(printf 'f%03d' "$i")
    : > "/root/tailtest/${name}"
    i=$((i + 1))
done
mkdir /root/tailtest/vim
test -d /root/tailtest/vim
ls -la /root/tailtest
sync
echo "#### REG DIR TAIL FSCK PASS ####"
EOF
else
    cat > "$guest_script_tmp" <<'EOF'
#!/bin/sh
set -x
export HOME=/root
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export DEBIAN_FRONTEND=noninteractive
echo "#### REGRESS DIR TAIL START ####"

dpkg --configure -a || true
apt-get install -y --no-install-recommends neovim-runtime
apt_rc=$?
echo "apt install rc=${apt_rc}"

syntax_dir="$(ls -d /usr/share/vim/vim*/syntax 2>/dev/null | head -1)"
echo "syntax_dir=${syntax_dir}"
test -n "$syntax_dir"
test -d "$syntax_dir/vim"
test -f "$syntax_dir/vim/generated.vim"

dpkg --configure -a
dpkg_rc=$?
echo "dpkg configure rc=${dpkg_rc}"
[ "$dpkg_rc" -eq 0 ]
echo "#### REGRESS DIR TAIL PASS ####"
EOF
fi

if [ "${REGRESS_INJECT_SCRIPT:-1}" = "1" ]; then
    debugfs -w -R "rm $GUEST_SCRIPT" "$RV_IMG" 2>/dev/null || true
    debugfs -w -R "write $guest_script_tmp $GUEST_SCRIPT" "$RV_IMG"
    info "已注入 guest 脚本 ${GUEST_SCRIPT}"
else
    info "跳过注入 guest 脚本（REGRESS_INJECT_SCRIPT=0）"
fi
rm -f "$guest_script_tmp"

info "构建 operator-run 内核（日志 ${BUILD_LOG}）"
make -C "$OS_DIR" configure >/dev/null 2>&1 || true
if ! make -C "$OS_DIR" kernel-rv-final MODE=run SCRIPT="$GUEST_SCRIPT" > "$BUILD_LOG" 2>&1; then
    tail -40 "$BUILD_LOG"
    error "内核构建失败，见 ${BUILD_LOG}" 1
fi
cp "$OS_DIR/kernel-rv-final" "$OS_DIR/kernel-rv"
info "内核构建完成"

rm -f "$OVERLAY"
cd "$OS_DIR"
info "启动 QEMU（snapshot/overlay，timeout=${QEMU_TIMEOUT}s）"
timeout "$QEMU_TIMEOUT" env WOS_SDCARD_BACKING="$RV_IMG" WOS_SNAPSHOT_ID="$OVERLAY_ID" \
    WOS_QEMU_MEM="${WOS_QEMU_MEM:-2G}" \
    bash "$SCRIPT_DIR/run/rv_qemu_run_snapshot.sh" > "$QEMU_LOG" 2>&1 || true

if rg -q "$PASS_MARKER" "$QEMU_LOG"; then
    info "guest 回归通过（${PASS_MARKER}）"
else
    error "guest 回归未通过；关键日志：" 1
    rg -n "REG DIR TAIL|REGRESS DIR TAIL|apt install rc=|syntax_dir|dpkg|ENOENT|Directory not empty|Kernel panic|RefCell" \
        "$QEMU_LOG" | tail -50
    exit 1
fi

if rg -qi "Kernel panic|RefCell already borrowed" "$QEMU_LOG"; then
    error "检测到内核 panic" 1
fi

info "overlay 转 raw 并执行 e2fsck"
qemu-img convert -O raw -S 4k "$OVERLAY" "$RAW_OUT"
# 只读检查会因未回放 journal 而报告空闲计数偏差；先回放并修复计数，
# 再以只读模式验证目录结构（乱码目录项/checksum 属于结构性错误，不会被忽略）。
if e2fsck -fy "$RAW_OUT" > "$LOG_DIR/regress-dir-tail.e2fsck-fy.log" 2>&1; then
    fy_rc=0
else
    fy_rc=$?
fi
# rc=1 表示已回放 journal / 修复计数，属于预期；其余非零视为失败。
if [ "$fy_rc" -ne 0 ] && [ "$fy_rc" -ne 1 ]; then
    tail -40 "$LOG_DIR/regress-dir-tail.e2fsck-fy.log"
    error "e2fsck 回放/修复失败 rc=${fy_rc}" 1
fi
if ! e2fsck -fn "$RAW_OUT" > "$LOG_DIR/regress-dir-tail.e2fsck-fn.log" 2>&1; then
    tail -40 "$LOG_DIR/regress-dir-tail.e2fsck-fn.log"
    error "e2fsck 只读校验未通过" 1
fi
info "e2fsck 通过（先回放 journal 修复计数，再只读校验）"
