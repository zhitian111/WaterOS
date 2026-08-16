#!/usr/bin/env bash
# 编译 WaterOS 决赛设计文档（XeLaTeX + latexmk + minted）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MAIN="main.tex"
OUT_DIR="build"
OUT_PDF="${OUT_DIR}/main.pdf"

usage() {
  cat <<'EOF'
用法: ./scripts/build.bash [命令]

命令:
  build   编译 PDF（默认，输出到 build/main.pdf）
  clean   删除 build/ 与根目录遗留的中间文件
  rebuild 先 clean 再 build
  watch   监听文件变更并自动重编（latexmk -pvc）

示例:
  ./scripts/build.bash
  ./scripts/build.bash watch
EOF
}

need_latexmk() {
  if ! command -v latexmk >/dev/null 2>&1; then
    echo "错误: 未找到 latexmk，请安装 TeX Live 并确保 latexmk 在 PATH 中。" >&2
    exit 1
  fi
  if ! command -v xelatex >/dev/null 2>&1; then
    echo "错误: 未找到 xelatex，请安装 texlive-xetex / texlive-langchinese。" >&2
    exit 1
  fi
  if ! command -v pygmentize >/dev/null 2>&1; then
    echo "错误: 未找到 pygmentize（minted 语法高亮需要）。" >&2
    echo "  Arch: sudo pacman -S python-pygments" >&2
    exit 1
  fi
  if command -v pacman >/dev/null 2>&1 && ! pacman -Q texlive-xetex &>/dev/null; then
    echo "错误: 未安装 texlive-xetex（提供 xelatex.fmt）。" >&2
    echo "  Arch: sudo pacman -S texlive-xetex texlive-langchinese" >&2
    exit 1
  fi
  if ! kpsewhich xelatex.fmt >/dev/null 2>&1; then
    echo "提示: 安装 texlive-xetex 后若仍无 xelatex.fmt，请执行:" >&2
    echo "  sudo fmtutil-sys --byfmt xelatex   # 或 fmtutil-user --byfmt xelatex" >&2
  fi
}

latexmk_common=(
  -outdir="$OUT_DIR"
  -auxdir="$OUT_DIR"
  -xelatex
  -shell-escape
  -interaction=nonstopmode
  -file-line-error
  -synctex=1
  -halt-on-error
)

ensure_out_dir() {
  mkdir -p "$OUT_DIR"
}

# 清理工程根目录下旧版「就地编译」遗留文件
clean_legacy_root_artifacts() {
  rm -rf _minted _minted-main _minted-*
  rm -f main.pdf main.log main.aux main.out main.toc main.thm main.xdv main.fls main.fdb_latexmk
  rm -f main.synctex.gz missfont.log
  rm -f chapters/**/*.aux chapters/*.aux 2>/dev/null || true
  find chapters -name '*.aux' -delete 2>/dev/null || true
  rm -f *.config.minted *.data.minted xelatex*.fls 2>/dev/null || true
}

do_build() {
  need_latexmk
  ensure_out_dir
  echo "==> 源码目录: $ROOT"
  echo "==> 输出目录: $ROOT/$OUT_DIR"
  echo "==> 引擎: xelatex + minted (latexmk -shell-escape)"
  latexmk "${latexmk_common[@]}" "$MAIN"
  if [[ -f "$OUT_PDF" ]]; then
    echo "==> 完成: $ROOT/$OUT_PDF"
  else
    echo "错误: 未生成 $OUT_PDF，请查看 $OUT_DIR/main.log。" >&2
    exit 1
  fi
}

do_clean() {
  need_latexmk
  latexmk -outdir="$OUT_DIR" -c "$MAIN" 2>/dev/null || true
  latexmk -outdir="$OUT_DIR" -C "$MAIN" 2>/dev/null || true
  rm -rf "$OUT_DIR"
  mkdir -p "$OUT_DIR"
  touch "$OUT_DIR/.gitkeep"
  clean_legacy_root_artifacts
  echo "==> 已清理 $OUT_DIR/ 与根目录遗留中间文件"
}

do_watch() {
  need_latexmk
  ensure_out_dir
  echo "==> 监听模式（Ctrl+C 退出），输出: $OUT_DIR/main.pdf"
  latexmk -pvc "${latexmk_common[@]}" "$MAIN"
}

cmd="${1:-build}"
case "$cmd" in
  build) do_build ;;
  clean) do_clean ;;
  rebuild) do_clean; do_build ;;
  watch) do_watch ;;
  -h|--help|help) usage ;;
  *)
    echo "未知命令: $cmd" >&2
    usage >&2
    exit 1
    ;;
esac
