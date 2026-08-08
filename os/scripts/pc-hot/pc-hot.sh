#!/usr/bin/env bash
# pc-hot: per-PC instruction counter (QEMU TCG plugin) + symbol aggregation.
#
# Arch-independent plugin source: pc-hot.c.  Per-arch entry points:
#   ./pc-hot-rv.sh ...   (RISC-V)
#   ./pc-hot-la.sh ...   (LoongArch)
#
# Usage:
#   pc-hot.sh <rv|la> build
#   pc-hot.sh <rv|la> run <pcs.txt> -- <qemu args...>
#   pc-hot.sh <rv|la> analyze <pcs.txt> [kernel.elf] [topN]
#   pc-hot.sh <rv|la> all <pcs.txt> [topN] -- <qemu args...>
#
# `run`/`all` append `-plugin file=$SO,out=<pcs.txt>` to your qemu command.
# Output is written only at exit: zero streaming output during the run.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"

usage() {
    cat <<'EOF'
usage:
  pc-hot.sh <rv|la> build
  pc-hot.sh <rv|la> run <pcs.txt> -- <qemu args...>
  pc-hot.sh <rv|la> analyze [-t <icount_shift>] <pcs.txt> [kernel.elf] [topN]
  pc-hot.sh <rv|la> all <pcs.txt> [topN] -- <qemu args...>

examples:
  ./scripts/pc-hot/pc-hot-rv.sh run /tmp/pcs-rv.txt -- \
      timeout 300 qemu-system-riscv64 -machine virt -kernel ./kernel-rv-final \
      -m 8G -nographic -smp 8 -bios default -no-reboot -icount shift=0,sleep=off
  ./scripts/pc-hot/pc-hot-la.sh all /tmp/pcs-la.txt 50 -- \
      timeout 300 qemu-system-loongarch64 -kernel ./kernel-la-final \
      -m 8G -nographic -smp 8 -no-reboot -icount shift=0,sleep=off

  # with -icount shift=N each executed instruction advances the virtual clock
  # by 2^N ns; add -t N to analyze to report virtual-clock ticks per core/symbol.
  ./scripts/pc-hot/pc-hot-rv.sh analyze -t 0 /tmp/pcs-rv.txt kernel-rv-final 50
EOF
}

ARCH="${1:-}"
if [ $# -gt 0 ]; then shift; fi
case "$ARCH" in
    rv|riscv)
        ARCH=rv
        DEFAULT_ELF=kernel-rv-final
        ;;
    la|loongarch64)
        ARCH=la
        DEFAULT_ELF=kernel-la-final
        ;;
    *)
        usage
        exit 2
        ;;
esac

CMD="${1:-}"
if [ $# -gt 0 ]; then shift; fi

BUILD_DIR="$HERE/build/$ARCH"
SO="$BUILD_DIR/pc-hot-$ARCH.so"
mkdir -p "$BUILD_DIR"

NM_TOOL="${PC_HOT_NM:-nm}"
ADDR2LINE_TOOL="${PC_HOT_ADDR2LINE:-addr2line}"

build() {
    local -a glib_cflags
    read -r -a glib_cflags <<< "$(pkg-config --cflags glib-2.0)"
    gcc "${glib_cflags[@]}" -shared -fPIC -O2 -o "$SO" "$HERE/pc-hot.c"
    echo "built: $SO"
}

ensure_so() {
    if [ ! -f "$SO" ]; then
        build
    fi
}

resolve_elf() {
    local elf="$1"
    if [ -f "$elf" ]; then
        echo "$elf"
        return
    fi
    if [ -f "$REPO_ROOT/$elf" ]; then
        echo "$REPO_ROOT/$elf"
        return
    fi
    echo "error: cannot find kernel ELF: $elf" >&2
    exit 1
}

run_qemu() {
    local out="${1:?pcs.txt output path required}"
    shift
    if [ "${1:-}" = "--" ]; then shift; fi
    ensure_so
    echo "[pc-hot] plugin=$SO out=$out" >&2
    exec "$@" -plugin "file=$SO,out=$out"
}

analyze() {
    local icount_shift="" quantum_ns=1
    while [ "${1:-}" = "-t" ] || [ "${1:-}" = "--icount-shift" ]; do
        icount_shift="$2"
        shift 2
    done
    if [ -n "$icount_shift" ]; then
        quantum_ns=$((1 << icount_shift))
    fi
    local pcs="${1:?pcs.txt required}"
    local elf
    elf="$(resolve_elf "${2:-$DEFAULT_ELF}")"
    local top="${3:-50}"
    local nm_file="$BUILD_DIR/nm.txt"
    local agg="$BUILD_DIR/fn-agg.txt"
    echo "[pc-hot] icount shift=${icount_shift:-off} (2^shift ns/insn)" >&2

    "$NM_TOOL" -n "$elf" > "$nm_file"
    awk '
        NR == FNR {
            if ($2 ~ /[Tt]/ && $3 !~ /^\.L/ && $3 != "$x" && $3 != "$d") {
                n++; a[n] = $1; s[n] = $3;
            }
            next;
        }
        ncol == "" && $1 != "#" { ncol = NF - 2; }
        {
            pc = $2;
            sub(/^0x/, "", pc);
            lo = 1; hi = n; best = 0;
            while (lo <= hi) {
                m = int((lo + hi) / 2);
                if (a[m] <= pc) { best = m; lo = m + 1 } else { hi = m - 1 }
            }
            fn = best ? s[best] : "??";
            if (!(fn in idx)) {
                idx[fn] = ++m2;
                fname[m2] = fn;
                spc[m2] = pc;
            }
            k = idx[fn];
            sum[k] += $1;
            for (v = 3; v <= NF; v++) pv[k, v] += $v;
        }
        END {
            for (i = 1; i <= m2; i++) {
                printf "%12d %s %s", sum[i], spc[i], fname[i];
                for (v = 3; v <= 2 + ncol; v++) printf " %d", pv[i, v];
                printf "\n";
            }
        }
    ' "$nm_file" "$pcs" | sort -rn > "$agg"
    echo "[pc-hot] $(wc -l < "$agg") symbols -> $agg" >&2

    if [ -n "$icount_shift" ]; then
        awk -v q="$quantum_ns" \
            '{for (v = 4; v <= NF; v++) c[v] += $v}
             END {
                 printf "# per-core virtual time (ms):";
                 t = 0;
                 for (v = 4; v <= NF; v++) {
                     x = c[v] * q / 1000000.0;
                     t += x;
                     printf " v%d=%.1f", v - 4, x;
                 }
                 printf " total=%.1f\n", t;
             }' "$agg"
    fi

    local rank=0
    head -n "$top" "$agg" | while read -r cnt pc fn rest; do
        rank=$((rank + 1))
        local d
        d="$("$ADDR2LINE_TOOL" -f -C -e "$elf" "0x$pc" 2>/dev/null | head -1 || true)"
        case "$d" in
            ""|.*) d="$fn" ;;
        esac
        if [ -n "$icount_shift" ]; then
            local ms
            ms="$(awk -v c="$cnt" -v q="$quantum_ns" \
                'BEGIN { printf "%.1f", c * q / 1000000.0 }')"
            printf '%4d %12s %10s ms  %s\n' "$rank" "$cnt" "$ms" "$d"
        else
            printf '%4d %12s  %s\n' "$rank" "$cnt" "$d"
        fi
    done
}

all() {
    local out="${1:?pcs.txt output path required}"
    local top="${2:-50}"
    shift 2
    if [ "${1:-}" = "--" ]; then shift; fi
    build
    local qemu_status=0
    "$@" -plugin "file=$SO,out=$out" || qemu_status=$?
    echo "[pc-hot] qemu exited with $qemu_status (timeout usually 124)" >&2
    analyze "$out" "$DEFAULT_ELF" "$top"
}

case "$CMD" in
    build)
        build
        ;;
    run)
        run_qemu "$@"
        ;;
    analyze)
        analyze "$@"
        ;;
    all)
        all "$@"
        ;;
    *)
        usage
        exit 2
        ;;
esac
