#!/usr/bin/env python3
"""Count unique functional-file patches across all local branch, remote and tag history."""
import argparse
import ast
import hashlib
import re
from collections import defaultdict
from pathlib import Path, PurePosixPath
import subprocess
import sys

AUTHORS = {
    '2367651943@qq.com': 'zhitian111',
    '1592858973@qq.com': 'kasss233',
    '2076567173@qq.com': 'cesllill',
    'lixianli@example.com': 'lixianlilili',
}
EXCLUDED_DIRS = {
    '.git', '.obsidian', '.codegraph', '.agent', '.agents', '.codex', '.vscode',
    'target', 'build', 'tmp', 'tem', 'mnt', '__pycache__', 'node_modules',
    'vendor', 'third_party', 'third-party', 'thirdparty', 'external',
    'docs', 'doc', 'exports', 'audits', 'history', 'logs', 'log', '成绩计算',
}
EXCLUDED_FILES = {'Cargo.lock', 'package-lock.json', 'feature-tree.txt', 'config.conf'}
CODE_SUFFIXES = {'.rs', '.c', '.h', '.cpp', '.hpp', '.cc', '.cxx', '.S', '.s',
                 '.asm', '.ld', '.lds', '.java', '.dts', '.dtsi'}
SCRIPT_SUFFIXES = {'.py', '.sh', '.bash', '.zsh', '.pl', '.awk', '.lua', '.js', '.ts', '.cmd'}
CONFIG_SUFFIXES = {'.toml', '.yaml', '.yml', '.json', '.conf', '.cfg', '.ini', '.mk', '.cmake', '.cnf', '.MF'}
CONFIG_NAMES = {'Makefile', 'makefile', 'GNUmakefile', 'Dockerfile', 'Containerfile',
                'CMakeLists.txt', 'Kconfig', '.gitignore', '.gitmodules', '.gitattributes',
                'rust-toolchain', '.config', 'PKGBUILD', 'Makefile.in', 'configure',
                'configure.ac', 'configure.in', 'meson.build', 'meson_options.txt'}
ROOTFS_CONFIG_NAMES = {'profile', 'hosts', 'inittab', 'passwd', 'group', 'wateros-release'}
BOOT_CONFIG_NAMES = {'jh7110-uEnv.txt', 'jh7110-vf2-uEnv.txt'}


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True,
                                   encoding='utf-8', errors='strict').strip()


def patch_path(value):
    # Git's quoted paths use octal UTF-8 bytes, including non-ASCII filenames.
    value = value.rstrip('\n').split('\t', 1)[0]
    if value.startswith('"'):
        value = ast.literal_eval(value).encode('latin1').decode('utf-8')
    return None if value == '/dev/null' else value[2:]


# Package recipes are our work; arbitrary source trees inside a package are not.
PACKAGE_OWN_SOURCES = {
    'user/packages/mgba/wateros/main.c',
    'user/packages/waterfm/wateros/main.c',
    'user/packages/operator-tools/src/syscall-transfer-smoke.c',
}
PACKAGE_OWN_ASSETS = {
    'user/packages/pacman/assets/pacman.conf',
    'user/packages/pacman/assets/mirrorlist',
    'user/packages/pacman/assets/archriscv-pacman',
    'user/packages/pacman/assets/archriscv-run',
}
UPSTREAM_CONFIGS = {'user/packages/busybox/config/wateros_defconfig'}


def user_owned_path(path):
    """Accept userland build integration, not imported application source."""
    p = PurePosixPath(path)
    if not path.startswith('user/'):
        return True
    if path in UPSTREAM_CONFIGS:
        return False
    if path.startswith('user/packages/'):
        parts = p.parts
        if len(parts) < 4:
            return False
        tail = parts[3:]
        if tail in {('build.py',), ('package.toml',)}:
            return True
        if path in PACKAGE_OWN_SOURCES or path in PACKAGE_OWN_ASSETS:
            return True
        if tail[0] in {'patches', 'scripts', 'tools', 'config'}:
            return True
        return parts[2] == 'openjdk21' and tail[0] == 'tests'
    return (len(p.parts) > 2 and p.parts[1] in {'tools', 'tests', 'configs', 'rootfs'}
            or path in {'user/Makefile', 'user/.gitignore'})


def changed_chars(path, text):
    """For patch artifacts count changed payload, not copied upstream context.

    Addition/deletion still describes changes to the maintained patch file,
    rather than applying or reversing the patch against an upstream repository.
    """
    if PurePosixPath(path).suffix in {'.patch', '.diff'}:
        if text.startswith(('+++', '---')) or text[:1] not in {'+', '-'}:
            return 0
        text = text[1:]
    return sum(not c.isspace() for c in text)


def category(path):
    p = PurePosixPath(path)
    if not user_owned_path(path):
        return None
    if (set(p.parts) & EXCLUDED_DIRS or p.name in EXCLUDED_FILES
            or any(part.startswith('ltp_log') for part in p.parts)):
        return None
    if p.suffix in {'.patch', '.diff'} and 'patches' in p.parts:
        return '移植补丁'
    if p.suffix in CODE_SUFFIXES:
        return '源码/测试'
    if p.suffix in SCRIPT_SUFFIXES:
        return '脚本'
    if not p.suffix and any(part in {'scripts', 'bin', 'sbin', 'init.d'} for part in p.parts):
        return '脚本'
    if ((path.startswith('user/rootfs/') and p.name in ROOTFS_CONFIG_NAMES)
            or (path.startswith('user/packages/') and 'config' in p.parts
                and (not p.suffix or p.name.endswith('_defconfig')))
            or (path.startswith('user/packages/pacman/assets/') and p.name == 'mirrorlist')
            or (path.startswith('user/packages/pacman/assets/')
                and p.name in {'archriscv-pacman', 'archriscv-run'})
            or p.name in BOOT_CONFIG_NAMES):
        return '运行/移植配置'
    if p.suffix in CONFIG_SUFFIXES or p.name in CONFIG_NAMES:
        return '配置'
    return None


def canonical_author(raw_name, email):
    if raw_name.strip().casefold() == 'outersystems':
        return None
    email = email.casefold()
    return email if email in AUTHORS else None


class Progress:
    """Render commit traversal progress on stderr, with readable redirected logs."""
    def __init__(self, total, stream=None):
        self.total = total
        self.stream = stream or sys.stderr
        self.interactive = self.stream.isatty()
        self.last = -1
        self(0)

    def __call__(self, done):
        percent = int(100 * done / self.total) if self.total else 100
        step = percent if self.interactive else percent // 10
        if step == self.last:
            return
        self.last = step
        width = 28
        filled = int(width * percent / 100)
        bar = '#' * filled + '-' * (width - filled)
        message = f'统计进度 [{bar}] {percent:3d}%  {done}/{self.total} 提交'
        print(('\r' if self.interactive else '') + message,
              end='\n' if not self.interactive or percent == 100 else '',
              file=self.stream, flush=True)


def collect(root, scope, revisions, progress=None):
    """Count each eligible functional patch once across the supplied ref snapshot.

    Duplicate patches retain the first eligible author in reverse topological
    traversal. Rewritten OuterSystems export history is excluded.
    """
    if isinstance(revisions, str):
        revisions = [revisions]
    totals = defaultdict(lambda: [0, 0, 0, 0])
    detail = defaultdict(lambda: [0, 0, 0, 0])
    names = dict(AUTHORS)
    stats = dict(commits=0, merges=0, excluded_authors=0, duplicates=0, counted=0)
    seen = set()
    author = None
    old = new = None
    file_patch = []
    patches = []
    changes = []
    in_hunk = False

    def selected(path):
        return (path is not None and category(path) is not None
                and (not scope or path == scope or path.startswith(scope + '/')))

    def finish_file():
        if author and (selected(old) or selected(new)):
            patches.extend(file_patch)
        file_patch.clear()

    def finish_commit():
        finish_file()
        if not author or not changes:
            patches.clear()
            changes.clear()
            return
        result = subprocess.run(['git', 'patch-id', '--stable'],
                                input=''.join(patches), text=True,
                                capture_output=True, check=True)
        if not result.stdout.strip():
            raise RuntimeError('无法计算非空功能补丁的 patch-id')
        patch_id = result.stdout.split()[0]
        if patch_id in seen:
            stats['duplicates'] += 1
        else:
            seen.add(patch_id)
            stats['counted'] += 1
            for group, offset, chars in changes:
                totals[author][offset] += 1
                totals[author][offset + 1] += chars
                detail[group, author][offset] += 1
                detail[group, author][offset + 1] += chars
        patches.clear()
        changes.clear()

    command = ['git', '-C', str(root), '-c', 'core.quotePath=true', 'log',
               '--reverse', '--topo-order', *revisions,
               '--format=%x00%H%x00%an%x00%aE%x00%P', '--root', '--no-ext-diff',
               '--no-textconv', '--no-color', '--diff-merges=off', '-p', '-U0',
               '-w', '--ignore-blank-lines', '--find-renames=50%']
    with subprocess.Popen(command, stdout=subprocess.PIPE, text=True,
                          encoding='utf-8', errors='replace') as process:
        for line in process.stdout:
            if re.match(r'^\x00[0-9a-f]{40,64}\x00', line):
                finish_commit()
                if progress:
                    progress(stats['commits'])
                _, sha, raw_name, email, parents = line.rstrip('\n').split('\0')
                author = canonical_author(raw_name, email)
                stats['commits'] += 1
                stats['merges'] += len(parents.split()) > 1
                stats['excluded_authors'] += author is None
                old = new = None
                in_hunk = False
            elif line.startswith('diff --git '):
                finish_file()
                old = new = None
                in_hunk = False
                file_patch.append(line)
            else:
                if file_patch:
                    file_patch.append(line)
                if not in_hunk and line.startswith('--- '):
                    old = patch_path(line[4:])
                elif not in_hunk and line.startswith('+++ '):
                    new = patch_path(line[4:])
                elif line.startswith('@@ '):
                    in_hunk = True
                elif author and in_hunk and line[:1] in {'+', '-'}:
                    path = new if line[0] == '+' else old
                    if not selected(path) or '\0' in line or '\ufffd' in line:
                        continue
                    chars = changed_chars(path, line[1:])
                    if chars:
                        area = '内核及内核工具' if path.startswith('os/') else (
                            '用户态及移植' if path.startswith('user/') else '仓库工具/配置')
                        changes.append((area + ' / ' + category(path), 0 if line[0] == '+' else 2, chars))
        finish_commit()
        if process.wait():
            raise RuntimeError('git log 执行失败')
        if progress:
            progress(stats['commits'])
    return totals, detail, names, stats


def share_bar(value, total, width=40):
    if not total or not value:
        return ''
    units = max(1, round(width * 8 * value / total))
    full, remainder = divmod(units, 8)
    return '█' * full + ('▏▎▍▌▋▊▉'[remainder - 1] if remainder else '')


def table(rows, names, details=False):
    sums = [sum(v[i] for v in rows.values()) for i in range(4)]

    def percent(value, total):
        return f'{100 * value / total:.2f}%' if total else '0.00%'

    if details:
        print('作者 | 新增行 | 新增字符 | 新增占比 | 删除行 | 删除字符 | 删除占比 | 总改动字符 | 总占比')
    else:
        print(f'{"作者":<14} {"贡献度":>7}  贡献度分布')
    for author, values in sorted(rows.items(), key=lambda item: (-item[1][1], item[0])):
        al, ac, dl, dc = values
        if details:
            print(f'{names[author]} | {al:,} | {ac:,} | {percent(ac, sums[1])} | '
                  f'{dl:,} | {dc:,} | {percent(dc, sums[3])} | {ac + dc:,} | '
                  f'{percent(ac + dc, sums[1] + sums[3])}')
        else:
            print(f'{names[author]:<16} {percent(ac, sums[1]):>10}  '
                  f'{share_bar(ac, sums[1])}')
    if details:
        print(f'合计新增字符：{sums[1]:,}')
        print(f'合计删除字符：{sums[3]:,}；总改动字符：{sums[1] + sums[3]:,}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', nargs='?', default=str(Path(__file__).resolve().parents[3]),
                        help='统计目录，显式相对路径相对于当前工作目录；默认整个仓库')
    parser.add_argument('--details', action='store_true',
                        help='显示新增/删除/总改动及领域、文件类别明细')
    args = parser.parse_args()
    root = Path(git(Path(__file__).resolve().parent, 'rev-parse', '--show-toplevel'))
    directory = Path(args.directory).resolve()
    try:
        scope = directory.relative_to(root).as_posix()
    except ValueError:
        parser.error('统计目录必须位于当前仓库内')
    scope = '' if scope == '.' else scope
    if git(root, 'rev-parse', '--is-shallow-repository') == 'true':
        parser.error('浅克隆缺少完整历史，请先补全历史')
    refs = git(root, 'for-each-ref', '--format=%(refname) %(objectname)',
               'refs/heads', 'refs/remotes', 'refs/tags').splitlines()
    excluded_refs = [line for line in refs if line.split()[0].startswith('refs/remotes/gitlab/')]
    refs = [line for line in refs if line not in excluded_refs]
    revisions = sorted({line.split()[1] for line in refs})
    if not revisions:
        parser.error('仓库没有分支或标签')
    snapshot = hashlib.sha256('\n'.join(sorted(refs)).encode()).hexdigest()[:16]
    if args.details:
        print(f'统计引用快照 {snapshot}；目录：{scope or "."}；引用 {len(refs)} 个；'
              f'排除 GitLab 导出引用 {len(excluded_refs)} 个。', flush=True)
        print('贡献度 = 有效新增字符占比；四位作者；排除 OuterSystems、vendor 和文档。', flush=True)
    commit_total = int(git(root, 'rev-list', '--count', *revisions))
    totals, detail, names, stats = collect(root, scope, revisions, Progress(commit_total))
    if args.details:
        print(f"遍历 {stats['commits']} 个提交；排除作者 {stats['excluded_authors']} 个；"
              f"合并提交 {stats['merges']} 个（冲突解决不计入）；重复功能补丁 {stats['duplicates']} 个；"
              f"计入 {stats['counted']} 个功能补丁。\n")
    print('贡献度')
    table(totals, names, args.details)
    if not args.details:
        return
    for area in sorted({key[0].split(' / ')[0] for key in detail}):
        rows = defaultdict(lambda: [0, 0, 0, 0])
        for (group, author), values in detail.items():
            if group.split(' / ')[0] == area:
                for i, value in enumerate(values):
                    rows[author][i] += value
        print(f'\n领域：{area}（占比以本领域为分母）')
        table(rows, names, details=True)
    for group in sorted({key[0] for key in detail}):
        print(f'\n分类：{group}（占比以本分类为分母）')
        table({author: values for (kind, author), values in detail.items() if kind == group},
              names, details=True)


if __name__ == '__main__':
    try:
        main()
    except (subprocess.CalledProcessError, RuntimeError, UnicodeError) as error:
        print(f'统计失败：{error}', file=sys.stderr)
        sys.exit(1)
