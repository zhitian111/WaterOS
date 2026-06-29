#!/usr/bin/env bash
# 检查 WaterOS LaTeX 文档编译环境（Arch Linux / TeX Live）
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok=0
warn=0
fail=0

say_ok()   { echo "  [OK]   $*"; ((ok++)) || true; }
say_warn() { echo "  [WARN] $*"; ((warn++)) || true; }
say_fail() { echo "  [FAIL] $*"; ((fail++)) || true; }

echo "=== WaterOS LaTeX 编译环境检查 ==="
echo "目录: $ROOT"
echo

# --- 引擎与工具 ---
echo ">> 编译工具"
for cmd in xelatex latexmk kpsewhich pygmentize; do
  if command -v "$cmd" >/dev/null 2>&1; then
    say_ok "$cmd → $(command -v "$cmd")"
  else
    say_fail "未找到 $cmd"
  fi
done

echo
echo ">> XeLaTeX 格式文件"
if kpsewhich xelatex.fmt >/dev/null 2>&1; then
  say_ok "kpsewhich xelatex.fmt → $(kpsewhich xelatex.fmt)"
elif [[ -f "${HOME}/.texlive/texmf-var/web2c/xetex/xelatex.fmt" ]]; then
  say_warn "kpsewhich 找不到 xelatex.fmt，但用户目录存在："
  echo "         ${HOME}/.texlive/texmf-var/web2c/xetex/xelatex.fmt"
  echo "         可执行: fmtutil-user --byfmt xelatex"
else
  say_fail "缺少 xelatex.fmt → 安装 texlive-xetex 后执行 fmtutil-sys --byfmt xelatex"
fi

if xelatex --version >/dev/null 2>&1; then
  say_ok "xelatex 可运行: $(xelatex --version | head -1)"
else
  say_fail "xelatex 无法启动"
fi

echo
echo ">> 推荐 TeX Live 包（Arch: pacman -S …）"
pkgs=(
  texlive-xetex
  texlive-langchinese
  texlive-langcjk
  texlive-latexextra
  texlive-latexrecommended
  texlive-pictures
  texlive-fontsrecommended
)
for pkg in "${pkgs[@]}"; do
  if command -v pacman >/dev/null 2>&1 && pacman -Q "$pkg" &>/dev/null; then
    say_ok "pacman: $pkg"
  elif command -v pacman >/dev/null 2>&1; then
    if [[ "$pkg" == "texlive-fontsrecommended" ]]; then
      say_warn "未安装 $pkg（Latin Modern 字体；本项目已用 fontset=none + Nimbus 回退，非必须）"
    else
      say_warn "未安装 $pkg"
    fi
  fi
done

echo
echo ">> 系统字体（fontconfig）"
check_font() {
  local name="$1"
  if fc-list ":family" 2>/dev/null | grep -qiF "$name"; then
    say_ok "字体: $name"
  else
    say_warn "未检测到: $name"
  fi
}
check_font "Noto Serif CJK SC"
check_font "Noto Sans CJK SC"
check_font "Nimbus Roman"
check_font "Nimbus Sans"
check_font "Nimbus Mono PS"
check_font "Times New Roman"
check_font "DejaVu Sans Mono"

echo
echo ">> 结论"
echo "  推荐编译方式: XeLaTeX + minted (latexmk -shell-escape)"
echo "  命令: ./scripts/build.bash"
echo
if (( fail > 0 )); then
  echo "存在 $fail 项失败、$warn 项警告。请先修复 [FAIL] 后再编译。"
  exit 1
fi
if (( warn > 0 )); then
  echo "环境可用（$warn 项警告）。可尝试: ./scripts/build.bash rebuild"
  exit 0
fi
echo "环境完整，可直接: ./scripts/build.bash"
exit 0
