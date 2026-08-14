# WaterOS 定向导出到 GitLab

[项目首页](../../../README.md) · [Agent 任务索引](README.md)

本说明用于把源仓库的指定提交或当前 `main`，定向同步到 GitLab 交付仓库。它是可直接交给
Agent 的操作流程，默认同步 `docs/` 和 `os/`，不迁移 `user/`、根目录 README 或任何未明确
纳入范围的路径。

> 安全边界：认证信息只能由运行环境、凭据助手或用户提供。不得把用户名、密码、token 或带
> 凭据的远程 URL 写入文档、提交信息、脚本或命令日志。

## 可直接复用的提示词

将下面文本发给 Agent，并按需替换尖括号参数：

```text
请执行 WaterOS 定向导出。完整遵循
docs/agents/tasks/export_to_gitlab.md。

源仓库：/home/zhitian/project/WaterOS_refactor
目标仓库：/home/zhitian/project/WaterOS_gitlab
源提交：<main | 完整或短 SHA>
同步范围：docs/ 和 os/
保留目标目录：docs/agents/、docs/build/、os/.cargo/、os/cargo-vendor/
不迁移：user/、根目录 README、其他根目录文件
目标作者：<name> <<email>>
推送：<是|否>

要求：先创建目标恢复分支；只同步源提交中受 Git 管理的文件；禁止把
export-ignore 当作文件同步依据；强制离线、无点目录构建通过后才推送。
成功时不要输出完整编译日志；失败时只输出足以定位错误的末尾片段。
```

## 目标与不变量

1. 目标的受管 `docs/` 和 `os/` 应与选定源提交一致。
2. 目标专用的离线依赖和运行时配置必须保留：`os/cargo-vendor/`、`os/.cargo/`。
3. `docs/agents/` 由目标仓库维护，不覆盖、不删除；`docs/build/` 若为本地生成物也不触碰。
4. 绝不导出源工作区中未提交的修改或 untracked 文件，除非用户明确要求并逐项列名。
5. 根目录 `Makefile`、`os/cargo-vendor-config.toml` 等不属于默认范围。只有用户显式要求时，
   才作为附加路径单独同步。
6. 所有目标导出提交使用目标仓库保存的 author name/email；源提交 author 不直接复用。导出
   提交的 author/committer 时间应使用源提交的 author 时间。
7. 推送前必须通过 `git diff --check` 和离线构建；运行期行为需要另行按 workload/QEMU 验证，
   不能用“能编译”替代。

## 0. 导出前检查

分别检查两个仓库，不得清理或覆盖用户已有修改：

```bash
git -C "$SOURCE_REPO" status --short
git -C "$TARGET_REPO" status --short
git -C "$SOURCE_REPO" cat-file -e "$SOURCE_REV^{commit}"
git -C "$SOURCE_REPO" show -s --format='%H%n%aI%n%an <%ae>%n%s' "$SOURCE_REV"
git -C "$TARGET_REPO" fetch gitlab main
git -C "$TARGET_REPO" rev-parse main
git -C "$TARGET_REPO" rev-parse gitlab/main
```

若目标本地 `main` 与 `gitlab/main` 不一致，先报告差异；不得擅自 rebase、reset 或 force-push。
若目标的受保护目录存在未跟踪文件，记录并继续，后续命令必须显式排除它们。

## 1. 创建恢复点

在目标仓库创建仅作恢复用途的本地分支；名称带时间戳且指向当前 `main`：

```bash
git -C "$TARGET_REPO" branch \
  "backup-before-export-$(date +%Y%m%d-%H%M%S)" main
```

不要删除该分支，直到用户确认交付结果符合预期。若迁移失败或验证失败，停止推送；用户要求
恢复时，以该分支为明确恢复点操作。

## 2. 构造“受管文件”清单

**不要使用 `git archive` 作为同步内容来源。** `git archive` 会遵守 `.gitattributes` 的
`export-ignore`，此前曾遗漏实际源码模块（例如 `cache.rs`、`cache_state.rs`），造成目标可提交
却无法编译。

以源提交的 Git tree 为权威文件集，排除 `docs/agents/`；以目标索引为删除依据，同时排除目标
保留目录：

```bash
git -C "$SOURCE_REPO" ls-tree -r --name-only "$SOURCE_REV" -- docs os \
  | rg -v '^docs/agents/' | sort > /tmp/wateros-export-source-files

git -C "$TARGET_REPO" ls-files -- docs os \
  | rg -v '^(docs/agents/|docs/build/|os/\.cargo/|os/cargo-vendor/)' \
  | sort > /tmp/wateros-export-target-files

comm -23 /tmp/wateros-export-target-files /tmp/wateros-export-source-files \
  > /tmp/wateros-export-delete-files
comm -23 /tmp/wateros-export-source-files /tmp/wateros-export-target-files \
  > /tmp/wateros-export-add-files
```

先展示删除清单和数量。删除列表应只包含目标已跟踪、但源提交不再存在的文件；若出现
`os/cargo-vendor/`、`os/.cargo/`、`docs/agents/` 或 `docs/build/`，立即停止并修正排除规则。

## 3. 同步内容

先删除精确清单中的目标受管文件，再从源**工作树的选定提交**导出同一份文件清单。最安全的
方式是建立临时 detached worktree，避免依赖当前工作区内容：

```bash
EXPORT_TREE=$(mktemp -d /tmp/wateros-export-tree.XXXXXX)
git -C "$SOURCE_REPO" worktree add --detach "$EXPORT_TREE" "$SOURCE_REV"

# 审核后执行：仅删除 /tmp/wateros-export-delete-files 中逐行列出的路径。
while IFS= read -r path; do
  git -C "$TARGET_REPO" rm -- "$path"
done < /tmp/wateros-export-delete-files

# 只复制源提交中已受管、且未被排除的文件；tar 不读取 .gitattributes 的 export-ignore。
tar -C "$EXPORT_TREE" -cf - -T /tmp/wateros-export-source-files \
  | tar -x -C "$TARGET_REPO"

git -C "$TARGET_REPO" add -A -- os docs \
  ':(exclude)os/.cargo' \
  ':(exclude)os/cargo-vendor' \
  ':(exclude)docs/agents' \
  ':(exclude)docs/build'
```

若源仓库的 `.gitignore` 有宽泛规则（例如匹配 `cache*.rs`），`git add` 可能拒绝实际受管源码。
用 `comm` 的 add 清单核对后，仅对明确缺失的源受管路径执行 `git add -f -- <path>`；不要对整个
目录使用 `-f`。

最后移除临时 worktree：

```bash
git -C "$SOURCE_REPO" worktree remove "$EXPORT_TREE"
```

## 4. 审查与提交

```bash
git -C "$TARGET_REPO" status --short
git -C "$TARGET_REPO" diff --cached --stat
git -C "$TARGET_REPO" diff --check
git -C "$TARGET_REPO" diff --check --cached
```

检查变更范围只含允许同步的 `docs/`、`os/`，以及用户显式附加的路径。提交信息应概述源提交
的实际主题，遵循 `[类别] 描述` 格式，且不得泄露自动化脚本、凭据或内部敏感信息。

```bash
GIT_AUTHOR_NAME="$TARGET_NAME" \
GIT_AUTHOR_EMAIL="$TARGET_EMAIL" \
GIT_COMMITTER_NAME="$TARGET_NAME" \
GIT_COMMITTER_EMAIL="$TARGET_EMAIL" \
GIT_AUTHOR_DATE="$SOURCE_AUTHOR_DATE" \
GIT_COMMITTER_DATE="$SOURCE_AUTHOR_DATE" \
git -C "$TARGET_REPO" commit -m '[sync] 同步 <源提交主题>'
```

如果前一次本地导出提交尚未推送且仅因遗漏受管文件需要修订，可在确认没有其他人提交后用
`git commit --amend --no-edit` 修订；已推送提交不得擅自改写历史。

## 5. 必做离线验证

验证模拟比赛环境：目标提交的归档副本、移除全部点目录、强制 Cargo offline，并通过根目录
Makefile 生成 `.cargo/config.toml`。成功时只输出简短状态；失败时输出末尾错误片段。

```bash
SIM_DIR=$(mktemp -d /tmp/wateros-online-sim.XXXXXX)
BUILD_DIR=$(mktemp -d /tmp/wateros-online-build.XXXXXX)

git -C "$TARGET_REPO" archive main | tar -x -C "$SIM_DIR"
find "$SIM_DIR" -depth -type d -name '.*' -exec rmdir {} + 2>/dev/null || true
mkdir -p "$SIM_DIR/os/target"
ln -s "$BUILD_DIR/riscv64gc-unknown-none-elf" \
  "$SIM_DIR/os/target/riscv64gc-unknown-none-elf"
ln -s "$BUILD_DIR/loongarch64-unknown-none" \
  "$SIM_DIR/os/target/loongarch64-unknown-none"

if ! CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$BUILD_DIR" \
  make -C "$SIM_DIR" all > /tmp/wateros-export-build.log 2>&1; then
  tail -80 /tmp/wateros-export-build.log
  exit 1
fi
test -f "$SIM_DIR/kernel-rv"
test -f "$SIM_DIR/kernel-la"
echo 'offline-build-ok'
```

这只证明交付版本可离线编译。若本次触及 VFS、exec、task、调度、MM、FS、IPC 或驱动，必须按
变更路径追加最小 QEMU/workload 回归；例如曾出现 `execve → close_cloexec_fds_for_current_task`
持有 I/O lease 而单核自旋的运行期卡死，此类问题无法由编译发现。

## 6. 推送与交付

仅在所有必要验证成功后推送。优先使用目标仓库配置好的凭据或凭据助手，推送命令中不得显示
密码：

```bash
git -C "$TARGET_REPO" push gitlab main:main
git -C "$TARGET_REPO" ls-remote gitlab refs/heads/main
git -C "$TARGET_REPO" rev-parse main
```

交付时报告：源 SHA、目标 SHA、同步范围、明确保留的目录、验证命令及结果、是否已推送、恢复
分支名称和任何未验证的运行期风险。不要在报告中回显凭据或完整编译日志。

## 附加范围

用户要求迁移下列文件时，须在提示词中逐项列出，并把它们加入 source/target 清单和暂存范围：

- 根目录 `Makefile`：用于在线平台创建 `os/.cargo/config.toml`。
- `os/cargo-vendor-config.toml`：不以点目录存在的 vendor 配置模板。
- 根目录 `.gitignore`：应合并目标已有的内核、镜像和 `target/` 忽略项，避免误将产物纳入版本库。

默认范围不包含这些附加路径，防止一次普通 `docs/os` 同步意外改写目标仓库入口。
