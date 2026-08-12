# WaterOS 品牌资源

[项目首页](../../../README.md) · [文档总览](../../README.md)

本目录保存 README、技术文档和演示材料使用的 WaterOS 品牌字标。

- `wateros-wordmark.txt`：ASCII 字标源文件；
- `wateros-wordmark.svg`：透明背景的青色矢量版本，供 Markdown 与 HTML 引用。

修改字标时应同步更新源文件和 SVG。SVG 由 Pango 将完整字符画作为一个等宽、左对齐的
文本块排版后转换为矢量路径，不为每一行手工设置坐标，也不依赖浏览器提供特定字体：

```bash
pango-view --no-display --backend=cairo \
  --background=transparent --foreground='#0891B2' \
  --font='Noto Sans Mono Bold 14px' --pixels --margin=0 --line-spacing=1 \
  --output=wateros-wordmark.svg wateros-wordmark.txt
```
