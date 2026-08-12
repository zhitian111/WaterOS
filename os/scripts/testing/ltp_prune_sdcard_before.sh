#!/usr/bin/env bash
# 用 debugfs 从 sdcard 镜像中删除 LTP 用例二进制，使 ltp_testcode.sh 从指定用例起跑。
#
# 删除规则与镜像内 ltp_testcode.sh 一致：对 ltp/testcases/bin/* 按字典序遍历，
# 删除所有 basename 严格小于 --before 的常规文件（保留 --before 本身及之后的用例）。
#
# 用法（在 os/ 目录）:
#   ./scripts/testing/ltp_prune_sdcard_before.sh
#   ./scripts/testing/ltp_prune_sdcard_before.sh --before mmapstress01
#   ./scripts/testing/ltp_prune_sdcard_before.sh --img sdcard-la.img --before open01 --libc glibc
#   ./scripts/testing/ltp_prune_sdcard_before.sh --dry-run --before mmapstress01
#   ./scripts/testing/ltp_prune_sdcard_before.sh --reset-from ../test_case/sdcard-la.img --before mmapstress01
#
# 选项:
#   --img PATH          目标镜像，默认 os/sdcard-la.img
#   --before NAME       保留此用例及之后；删除字典序更小的用例，默认 mmapstress01
#   --libc glibc|musl|both  处理哪套 libc 树，默认 both
#   --dry-run           只列出将删除的用例，不写镜像
#   --reset-from PATH   先 cp 源镜像到 --img，再裁剪
#   --list              同 --dry-run

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

IMG="$ROOT/sdcard-la.img"
BEFORE="unlink08"
LIBC="both"
DRY_RUN=0
RESET_FROM=""

usage() {
    sed -n '2,20p' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --img)
            shift
            IMG="${1:-}"
            [ -n "$IMG" ] || { echo "missing value for --img" >&2; exit 2; }
            [[ "$IMG" = /* ]] || IMG="$ROOT/$IMG"
            shift
            ;;
        --img=*)
            IMG="${1#--img=}"
            [[ "$IMG" = /* ]] || IMG="$ROOT/$IMG"
            shift
            ;;
        --before)
            shift
            BEFORE="${1:-}"
            [ -n "$BEFORE" ] || { echo "missing value for --before" >&2; exit 2; }
            shift
            ;;
        --before=*)
            BEFORE="${1#--before=}"
            shift
            ;;
        --libc)
            shift
            LIBC="${1:-}"
            shift
            ;;
        --libc=*)
            LIBC="${1#--libc=}"
            shift
            ;;
        --reset-from)
            shift
            RESET_FROM="${1:-}"
            [ -n "$RESET_FROM" ] || { echo "missing value for --reset-from" >&2; exit 2; }
            [[ "$RESET_FROM" = /* ]] || RESET_FROM="$ROOT/$RESET_FROM"
            shift
            ;;
        --reset-from=*)
            RESET_FROM="${1#--reset-from=}"
            [[ "$RESET_FROM" = /* ]] || RESET_FROM="$ROOT/$RESET_FROM"
            shift
            ;;
        --dry-run|--list) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$LIBC" in
    glibc|musl|both) ;;
    *) echo "invalid --libc: $LIBC (want glibc|musl|both)" >&2; exit 2 ;;
esac

if ! command -v debugfs >/dev/null 2>&1; then
    echo "debugfs not found (install e2fsprogs)" >&2
    exit 1
fi

log() { printf '[ltp-prune] %s\n' "$*"; }

if [ -n "$RESET_FROM" ]; then
    [ -f "$RESET_FROM" ] || { echo "reset source not found: $RESET_FROM" >&2; exit 1; }
    if [ "$DRY_RUN" -eq 1 ]; then
        log "dry-run: would reset image $RESET_FROM -> $IMG"
        IMG="$RESET_FROM"
    else
        log "reset image: $RESET_FROM -> $IMG"
        cp -f "$RESET_FROM" "$IMG"
    fi
fi

[ -f "$IMG" ] || { echo "image not found: $IMG" >&2; exit 1; }

LIBC_PREFIXES=()
case "$LIBC" in
    glibc) LIBC_PREFIXES=(glibc) ;;
    musl) LIBC_PREFIXES=(musl) ;;
    both) LIBC_PREFIXES=(glibc musl) ;;
esac

mapfile -t TO_DELETE < <(
    python3 - "$IMG" "$BEFORE" "${LIBC_PREFIXES[@]}" <<'PY'
import re
import subprocess
import sys

img = sys.argv[1]
before = sys.argv[2]
prefixes = sys.argv[3:]

def list_binaries(prefix: str) -> list[str]:
    remote = f"/{prefix}/ltp/testcases/bin"
    try:
        out = subprocess.check_output(
            ["debugfs", "-R", f"ls {remote}", img],
            stderr=subprocess.STDOUT,
            text=True,
        )
    except subprocess.CalledProcessError as exc:
        print(f"error: debugfs ls {remote} failed:\n{exc.output}", file=sys.stderr)
        sys.exit(1)
    names: list[str] = []
    for line in out.splitlines():
        if line.startswith("debugfs") or not line.strip():
            continue
        if "Directory block checksum does not match" in line:
            print(
                f"warning: {remote} may be corrupted; consider --reset-from test_case/sdcard-*.img",
                file=sys.stderr,
            )
        for match in re.finditer(r"\(\d+\)\s+(\S+)", line):
            name = match.group(1)
            if name not in (".", ".."):
                names.append(name)
    return sorted(set(names))

seen: set[tuple[str, str]] = set()
for prefix in prefixes:
    names = list_binaries(prefix)
    if before not in names:
        print(
            f"warning: {before} not found under /{prefix}/ltp/testcases/bin "
            f"({len(names)} entries); deleting all names < {before!r}",
            file=sys.stderr,
        )
    for name in names:
        if name < before:
            key = (prefix, name)
            if key not in seen:
                seen.add(key)
                print(f"{prefix}\t{name}")
PY
)

if [ "${#TO_DELETE[@]}" -eq 0 ]; then
    log "nothing to delete before '$BEFORE' in $IMG ($LIBC)"
    exit 0
fi

log "image: $IMG"
log "before: $BEFORE (keep this case and later, per glob sort order)"
log "libc: $LIBC"
log "candidates: ${#TO_DELETE[@]}"

if [ "$DRY_RUN" -eq 1 ]; then
    printf '%s\n' "${TO_DELETE[@]}" | sed 's/^/  /'
    exit 0
fi

tmp_cmds="$(mktemp)"
trap 'rm -f "$tmp_cmds"' EXIT
{
    for entry in "${TO_DELETE[@]}"; do
        prefix="${entry%%$'\t'*}"
        name="${entry#*$'\t'}"
        printf 'rm /%s/ltp/testcases/bin/%s\n' "$prefix" "$name"
    done
} >"$tmp_cmds"

log "debugfs batch rm (${#TO_DELETE[@]} files) ..."
debugfs -w -f "$tmp_cmds" "$IMG" >/dev/null

log "done. next run should start at or after '$BEFORE' (skipped basenames may still fast-exit in kernel)."
