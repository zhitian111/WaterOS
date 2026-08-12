#!/usr/bin/env bash
# pc-hot：通过 QEMU TCG plugin 逐 PC 统计指令数，并按符号聚合结果。
#
# pc-hot.c 是架构无关的插件源码；按架构使用以下入口：
#   ./pc-hot-rv.sh ...   (RISC-V)
#   ./pc-hot-la.sh ...   (LoongArch)
#
# 用法：
#   pc-hot.sh <rv|la> build
#   pc-hot.sh <rv|la> run <pcs.txt> -- <qemu args...>
#   pc-hot.sh <rv|la> analyze <pcs.txt> [kernel.elf] [topN]
#   pc-hot.sh <rv|la> all <pcs.txt> [topN] -- <qemu args...>
#
# `run` 和 `all` 会在 QEMU 命令末尾追加 `-plugin file=$SO,out=<pcs.txt>`。
# 插件只在退出时写入结果，运行期间不会流式输出统计数据。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
WOS_LOG_COMPONENT=PC-HOT
source "$HERE/../source/console.bash"

usage() {
    cat <<'EOF'
用法:
  pc-hot.sh <rv|la> build
  pc-hot.sh <rv|la> run <pcs.txt> -- <qemu args...>
  pc-hot.sh <rv|la> analyze [-t <icount_shift>] <pcs.txt> [kernel.elf] [topN]
  pc-hot.sh <rv|la> all <pcs.txt> [topN] -- <qemu args...>

示例:
  ./scripts/pc-hot/pc-hot-rv.sh run /tmp/pcs-rv.txt -- \
      timeout 300 qemu-system-riscv64 -machine virt -kernel ./kernel-rv-final \
      -m 8G -nographic -smp 8 -bios default -no-reboot -icount shift=0,sleep=off
  ./scripts/pc-hot/pc-hot-la.sh all /tmp/pcs-la.txt 50 -- \
      timeout 300 qemu-system-loongarch64 -kernel ./kernel-la-final \
      -m 8G -nographic -smp 8 -no-reboot -icount shift=0,sleep=off

  # 使用 -icount shift=N 时，每条指令使虚拟时钟前进 2^N ns；分析时传入
  # -t N，可按核心和符号报告对应的虚拟时钟时间。
  ./scripts/pc-hot/pc-hot-rv.sh analyze -t 0 /tmp/pcs-rv.txt kernel-rv-final 50
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

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
if [[ "$CMD" == "-h" || "$CMD" == "--help" || "$CMD" == "help" ]]; then
    usage
    exit 0
fi

BUILD_DIR="$HERE/build/$ARCH"
SO="$BUILD_DIR/pc-hot-$ARCH.so"
mkdir -p "$BUILD_DIR"

NM_TOOL="${PC_HOT_NM:-nm}"
ADDR2LINE_TOOL="${PC_HOT_ADDR2LINE:-addr2line}"

build() {
    local -a glib_cflags
    read -r -a glib_cflags <<< "$(pkg-config --cflags glib-2.0)"
    gcc "${glib_cflags[@]}" -shared -fPIC -O2 -o "$SO" "$HERE/pc-hot.c"
    info "QEMU PC 热点插件已构建 path=${SO}"
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
    error "内核 ELF 不存在 path=${elf}" 1
}

run_qemu() {
    local out="${1:?必须提供 pcs.txt 输出路径}"
    shift
    if [ "${1:-}" = "--" ]; then shift; fi
    ensure_so
    info "开始采集 PC 热点 plugin=${SO} output=${out}"
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
    local pcs="${1:?必须提供 pcs.txt 路径}"
    local elf
    elf="$(resolve_elf "${2:-$DEFAULT_ELF}")"
    local top="${3:-50}"
    local nm_file="$BUILD_DIR/nm.txt"
    local agg="$BUILD_DIR/fn-agg.txt"
    info "设置 QEMU 指令计数 shift=${icount_shift:-off} unit=2^shift_ns_per_instruction"

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
    info "符号热点聚合完成 symbols=$(wc -l < "$agg") output=${agg}"

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
    local out="${1:?必须提供 pcs.txt 输出路径}"
    local top="${2:-50}"
    shift 2
    if [ "${1:-}" = "--" ]; then shift; fi
    build
    local qemu_status=0
    "$@" -plugin "file=$SO,out=$out" || qemu_status=$?
    if (( qemu_status == 0 )); then
        info "QEMU 采集正常结束 exit_code=${qemu_status}"
    else
        warning "QEMU 采集结束 exit_code=${qemu_status} timeout_exit_code=124"
    fi
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
