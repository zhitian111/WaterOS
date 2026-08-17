# WaterOS 图表资源

[项目首页](../../../README.md) · [文档总览](../../README.md) · [系统架构](../../../README.md#系统架构)

本目录保存 README、技术文档和演示材料共用的图表。Mermaid 源文件与生成的 SVG
使用相同的文件名，README 只引用 SVG，以保证 GitHub、GitLab 和本地预览的显示一致。

## 文件约定

- `mermaid-theme.mmd`：所有 Mermaid 图共用的节点样式与 WaterOS 配色。
- `mermaid-config.json`：本地 Mermaid CLI 使用的通用布局和字体配置。
- `*.mmd`：图表源码，只描述节点、连线和样式分类。
- `*.svg`：供 Markdown、LaTeX 和演示材料引用的渲染结果。

其中 `wateros-presentation-architecture.svg` 是面向五分钟决赛展示的 16:9 分层图，按
“用户态 → ABI → 共享内核服务 → 设备与持久化 → 双架构底座”组织；它与
`wateros-architecture.svg` 的组件依赖视图互补，不替代后者。

配色以 Radix Colors 的 Ruby 色阶为基础，使用浅色背景与 Ruby 12 深红文字组成固定
对比，并以山东大学红 `#9E1B32` 表示关键边界。SVG 画布透明，但容器和节点保留背景，
因此标题与正文在浅色、深色页面中均可辨认。

## 在线渲染

无需安装 Mermaid CLI。脚本会将图源与共享样式合并，并交给 mermaid.ink 的公开
渲染服务生成 SVG：

```bash
./render-online.sh wateros-architecture
```

## 本地渲染

安装 Mermaid CLI 后可使用：

```bash
mmdc \
  --input wateros-architecture.mmd \
  --output wateros-architecture.svg \
  --configFile mermaid-config.json
```

本地 CLI 不会自动读取 `mermaid-theme.mmd`。需要完全复现仓库配色时，使用在线渲染
脚本，或先将共享样式追加到待渲染的临时源文件。
