# WaterOS Commit 规范

与 [WORKFLOW.md](./WORKFLOW.md) 中的「Commit 规范」一致，此处给出快速参考与示例，便于复制粘贴。

## 格式

```
<type>(<scope>): <subject>
```

- **type**：见下表
- **scope**：组件或子模块，如 `fs`, `driver-block`, `ipc-pipe`
- **subject**：简短祈使句，首字母小写，无句号

## type 速查

| type    | 用途           |
|---------|----------------|
| `feat`  | 新功能（含新实现） |
| `fix`   | Bug 修复       |
| `refactor` | 重构（行为不变） |
| `docs`  | 仅文档         |
| `test`  | 测试相关       |
| `chore` | 构建/脚本/配置  |
| `api`   | API 定义变更（核心开发） |

## 示例

```bash
feat(fs): implement devfs root and device nodes
fix(ipc-pipe): correct buffer boundary in blocking read
api(driver-block): add sector_alignment to BlockDevice
docs(workflow): add branch naming examples
chore: update rust toolchain in Makefile
```

## 多行 body（可选）

```
feat(vfs): add inode cache

- lru cache for inode lookup
- configurable capacity via feature
```

提交前建议运行：

```bash
cargo fmt
cargo clippy
```
