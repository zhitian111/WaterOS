"""Exercise historical statistics against a real, isolated Git repository."""
import importlib.util
import io
from contextlib import redirect_stdout
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    'contribution_stats', Path(__file__).resolve().parents[1] / 'maintenance/stat_contribute.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class HistoryStatsTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git('init', '-b', 'main')
        (self.root / 'os').mkdir()

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args],
                                       stderr=subprocess.DEVNULL, text=True).strip()

    def commit(self, name, email=None):
        email = email or next(e for e, n in m.AUTHORS.items() if n == name)
        self.git('add', '.')
        self.git('-c', f'user.name={name}', '-c', f'user.email={email}', 'commit', '-m', name)
        return self.git('rev-parse', 'HEAD')

    def collect(self, refs='main', scope='os'):
        return m.collect(self.root, scope, refs)

    def test_unicode_whitespace_rename_delete_and_exclusions(self):
        f = self.root / 'os/中文 空格.rs'
        f.write_text('你 好\nabc\n')
        self.commit('zhitian111')
        f.write_text('你好\nabd\n')
        self.commit('kasss233')
        self.git('mv', 'os/中文 空格.rs', 'os/renamed.rs')
        self.commit('cesllill')
        (self.root / 'os/renamed.rs').unlink()
        self.commit('cesllill')
        for path in ['os/vendor/lib/src/lib.rs', 'os/third_party/lib.c',
                     'os/README.md', 'os/ltp_log_final/data.json',
                     'os/docs/example.py', 'os/Cargo.lock', 'os/picture.drawio']:
            p = self.root / path
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('generated or third party\n')
        self.commit('zhitian111')
        (self.root / 'os/vendor/lib/src/lib.rs').write_text('maintenance\n')
        self.commit('zhitian111')
        totals, _, _, stats = self.collect()
        self.assertEqual(totals['2367651943@qq.com'], [2, 5, 0, 0])
        self.assertEqual(totals['1592858973@qq.com'], [1, 3, 1, 3])
        self.assertEqual(totals['2076567173@qq.com'], [0, 0, 2, 5])
        self.assertEqual(stats['counted'], 3)
        self.assertFalse(self.collect(scope='other')[0])

    def test_userland_porting_file_rules(self):
        for path in ['user/packages/demo/patches/fix.patch',
                     'user/packages/openjdk21/tests/Hello.java',
                     'user/packages/demo/scripts/start-demo',
                     'user/rootfs/base/etc/init.d/rcS', 'user/rootfs/base/etc/inittab',
                     'user/packages/pacman/assets/mirrorlist',
                     'user/packages/pacman/assets/archriscv-run',
                     'os/scripts/root_image/board.dts',
                     'os/scripts/root_image/boot-board.cmd',
                     'os/scripts/root_image/jh7110-uEnv.txt']:
            self.assertIsNotNone(m.category(path), path)
        for path in ['user/vendor/lib/patches/fix.patch',
                     'user/packages/openjdk21/tests/Hello.class.b64',
                     'user/packages/mgba/roms/demo.gba.xz',
                     'user/packages/demo/assets/README.md', '成绩计算/calc.py',
                     'user/packages/demo/src/upstream.c',
                     'user/packages/demo/upstream/Makefile',
                     'user/packages/demo/assets/copied.json',
                     'user/packages/busybox/config/wateros_defconfig']:
            self.assertIsNone(m.category(path), path)
        (self.root / 'user/packages/demo/patches').mkdir(parents=True)
        (self.root / 'user/packages/demo/patches/fix.patch').write_text(
            '--- a/demo.c\n+++ b/demo.c\n@@ -1 +1 @@\n upstream context\n-old\n+new\n')
        (self.root / 'user/packages/demo/src').mkdir()
        (self.root / 'user/packages/demo/src/upstream.c').write_text('copied upstream code\n')
        self.commit('zhitian111')
        totals, detail, _, _ = self.collect(scope='user')
        self.assertEqual(totals['2367651943@qq.com'], [2, 6, 0, 0])
        self.assertEqual(m.changed_chars('user/packages/demo/patches/fix.patch', ' context'), 0)
        self.assertEqual(m.changed_chars('user/packages/demo/patches/fix.patch', '+++ b/main.c'), 0)
        self.assertEqual(m.changed_chars('user/packages/demo/patches/fix.patch', '+our code'), 7)
        for owned in m.PACKAGE_OWN_SOURCES | m.PACKAGE_OWN_ASSETS:
            self.assertIsNotNone(m.category(owned), owned)
        self.assertIn(('用户态及移植 / 移植补丁', '2367651943@qq.com'), detail)

    def test_all_branches_patch_dedup_and_author_filter(self):
        (self.root / 'os/base.rs').write_text('base\n')
        base = self.commit('zhitian111')
        self.git('switch', '-c', 'feature')
        (self.root / 'os/branch.rs').write_text('xyz\n')
        branch = self.commit('kasss233')
        self.git('switch', 'main')
        self.git('-c', 'user.name=cesllill', '-c', 'user.email=2076567173@qq.com',
                 'cherry-pick', branch)
        totals, _, _, stats = self.collect(['main', 'feature'])
        self.assertEqual(sum(v[1] for v in totals.values()), 7)
        self.assertEqual(stats['duplicates'], 1)
        self.git('switch', '-c', 'unmerged', base)
        (self.root / 'os/extra.rs').write_text('extra\n')
        self.commit('cesllill')
        (self.root / 'os/export.rs').write_text('not a team contribution\n')
        self.commit('OuterSystems', 'T202610422999926@eduxiji.net')
        (self.root / 'os/unknown.rs').write_text('outsider\n')
        self.commit('Other', 'other@example.com')
        totals, _, _, stats = self.collect(['main', 'feature', 'unmerged'])
        self.assertEqual(sum(v[1] for v in totals.values()),
                         12)
        self.assertEqual(stats['excluded_authors'], 2)
        self.git('switch', 'main')
        self.git('-c', 'user.name=zhitian111', '-c', 'user.email=2367651943@qq.com',
                 'merge', '--no-ff', 'feature', '-m', 'merge')
        self.assertEqual(self.collect(['main', 'unmerged'])[3]['merges'], 1)

    def test_export_duplicate_fourth_author_and_display(self):
        (self.root / 'os/base.rs').write_text('base\n')
        base = self.commit('zhitian111')
        (self.root / 'os/feature.rs').write_text('feature\n')
        feature = self.commit('kasss233')
        self.git('switch', '-c', 'export', base)
        self.git('-c', 'user.name=OuterSystems',
                 '-c', 'user.email=T202610422999926@eduxiji.net', 'cherry-pick', feature)
        self.git('-c', 'user.name=OuterSystems',
                 '-c', 'user.email=T202610422999926@eduxiji.net',
                 'commit', '--amend', '--no-edit',
                 '--author=OuterSystems <T202610422999926@eduxiji.net>')
        (self.root / 'os/fourth.rs').write_text('tiny\n')
        self.commit('lixianlilili')
        progress = []
        totals, _, names, stats = m.collect(self.root, 'os', ['main', 'export'], progress.append)
        self.assertEqual(totals['2367651943@qq.com'][1], 4)
        self.assertEqual(totals['1592858973@qq.com'][1], 7)
        self.assertEqual(totals['lixianli@example.com'][1], 4)
        self.assertEqual(stats['duplicates'], 0)
        self.assertEqual(stats['excluded_authors'], 1)
        self.assertEqual(progress[-1], stats['commits'])
        self.assertEqual(progress, sorted(progress))
        output = io.StringIO()
        with redirect_stdout(output):
            m.table(totals, names)
        self.assertIn('贡献度', output.getvalue())
        self.assertNotIn('新增', output.getvalue())
        self.assertNotIn('删除', output.getvalue())
        self.assertNotIn('总改动', output.getvalue())
        self.assertIn('lixianlilili', output.getvalue())
        self.assertIn('█', output.getvalue())
        self.assertTrue(m.share_bar(1, 100000))
        status = io.StringIO()
        bar = m.Progress(4, status)
        bar(4)
        self.assertIn('100%', status.getvalue())
        self.assertIn('4/4', status.getvalue())


if __name__ == '__main__':
    unittest.main()
