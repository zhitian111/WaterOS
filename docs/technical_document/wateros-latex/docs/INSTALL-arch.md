# Arch Linux 编译依赖

## 最小安装（推荐）

你已安装 `texlive-basic`、`texlive-latex`、`texlive-binextra`（含 `latexmk`）。  
**缺的是 XeLaTeX 与中文**，因此会报 `can't find xelatex.fmt`：

```bash
sudo pacman -S texlive-xetex texlive-langchinese \
  texlive-latexrecommended texlive-latexextra
```

| 包 | 作用 |
|----|------|
| `texlive-xetex` | **xelatex**、`fontspec`、生成 `xelatex.fmt` |
| `texlive-langchinese` | `ctexbook`、中文排版 |
| `texlive-latexrecommended` | `geometry`、`hyperref`、`fancyhdr` 等常用宏包 |
| `texlive-latexextra` | `subcaption`、`titletoc`、`listings` 等 |

安装后生成格式文件（二选一）：

```bash
sudo fmtutil-sys --byfmt xelatex
# 若无 root / 仅当前用户：
fmtutil-user --byfmt xelatex
```

## 字体（纯 Linux，非 WSL 读 Windows 字体时）

```bash
sudo pacman -S noto-fonts-cjk
```

文档在找不到 `C:/Windows/Fonts` 时会回退到 Noto CJK 与 `DejaVu Sans Mono`。

## 一键装全（体积大，省心）

```bash
sudo pacman -S texlive-most noto-fonts-cjk
sudo fmtutil-sys --all
```

## 编译

```bash
cd docs/technical_document/wateros-latex
./scripts/build.bash
```

输出：`build/main.pdf`

## 可选

- `biber`：若以后改用 biblatex（当前用 `natbib` + `.bib`，一般不需要）
- `tex-gyre`：西文字体（TeX Gyre，通常已随 texlive 带上）
