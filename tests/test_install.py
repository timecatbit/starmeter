"""Installer staging and rollback, without SwiftBar or a real plugin folder."""
from contextlib import redirect_stdout
import errno
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'));sys.path.insert(0,str(ROOT/'lib'))
spec=importlib.util.spec_from_file_location('install_script',ROOT/'scripts/install.py')
install=importlib.util.module_from_spec(spec);spec.loader.exec_module(install)
import swiftbar_config


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.dest=Path(self.temp.name)/'plugins';self.panel=self.dest/'.opencodex-panel'
        (self.panel/'lib').mkdir(parents=True);(self.panel/'lib/old.py').write_text('old')
        (self.panel/'OpenCodexPanel.app').mkdir();(self.panel/'OpenCodexPanel.app/old-file').write_text('old')
        # A package tree of its own: the real helper isn't built yet when the unit tests run.
        package=Path(self.temp.name)/'package'
        for name in ('build/OpenCodexPanel.app/Contents/Info.plist','build/starmeter-engine','assets/logo.svg','lib/model-pricing.json',
                     'lib/CCUSAGE_LICENSE','lib/swiftbar_plugin.py'):
            (package/name).parent.mkdir(parents=True,exist_ok=True);(package/name).write_text('new')
        root=patch.object(install,'ROOT',package);root.start();self.addCleanup(root.stop)
        self.signature_ok=True;real=subprocess.run
        def run(command,*args,**kwargs):
            if command[0]=='codesign':
                if not self.signature_ok:raise subprocess.CalledProcessError(1,command)
                return subprocess.CompletedProcess(command,0)
            return real(command,*args,**kwargs)
        signed=patch.object(install.subprocess,'run',side_effect=run);signed.start();self.addCleanup(signed.stop)
    def test_a_helper_that_fails_its_signature_check_changes_nothing(self):
        self.signature_ok=False
        with self.assertRaises(subprocess.CalledProcessError):install.stage(self.dest)
        self.assertEqual(sorted(p.name for p in self.panel.iterdir()),['OpenCodexPanel.app','lib'])
        self.assertEqual((self.panel/'OpenCodexPanel.app/old-file').read_text(),'old')
    def test_parts_are_replaced_whole_and_put_back_when_a_later_step_fails(self):
        staging=install.stage(self.dest)
        # lib/ carries only what the engine reads; the engine goes in bin/.
        self.assertEqual(sorted(p.name for p in (staging/'lib').iterdir()),['CCUSAGE_LICENSE','model-pricing.json'])
        self.assertTrue((staging/'bin/starmeter-engine').is_file())
        pairs=[(staging/'OpenCodexPanel.app',self.panel/'OpenCodexPanel.app'),(staging/'lib',self.panel/'lib'),
               (staging/'bin',self.panel/'bin')]
        moved=[];install.swap(pairs,staging,moved)
        # Whole parts: a file the new version dropped doesn't linger beside it.
        self.assertFalse((self.panel/'lib/old.py').exists());self.assertFalse((self.panel/'OpenCodexPanel.app/old-file').exists())
        install.restore(moved)
        self.assertEqual((self.panel/'lib/old.py').read_text(),'old');self.assertEqual((self.panel/'OpenCodexPanel.app/old-file').read_text(),'old')
        self.assertFalse((self.panel/'bin').exists())  # It wasn't there before.
        self.assertFalse((self.panel/'lib/model-pricing.json').exists())
    def test_a_rename_that_fails_midway_undoes_the_parts_already_moved(self):
        staging=install.stage(self.dest)
        pairs=[(staging/'lib',self.panel/'lib'),(staging/'missing',self.panel/'OpenCodexPanel.app')]
        moved=[]
        with self.assertRaises(FileNotFoundError):install.swap(pairs,staging,moved)
        install.restore(moved)
        self.assertEqual((self.panel/'lib/old.py').read_text(),'old');self.assertEqual((self.panel/'OpenCodexPanel.app/old-file').read_text(),'old')

    def backups(self,names):
        root=Path(self.temp.name)/'backups'
        for name in names:(root/name).mkdir(parents=True)
        return root
    def test_only_the_newest_install_backups_are_kept(self):
        old=['20261101-10000%d-abcde%d'%(i,i) for i in range(7)]+['20991231-235959-ffffff']
        others=['removed-plugins-20261105-143627','configuration-cleanup-20261105-144939','notes']
        root=self.backups(old+others)
        (root/'20261101-090000-aaaaaa.zip').write_text('a file, not a backup')
        outside=Path(self.temp.name)/'elsewhere';outside.mkdir();(outside/'keep').write_text('kept')
        (root/'20261101-080000-bbbbbb').symlink_to(outside,target_is_directory=True)
        (self.dest/'starmeter.30s.sh').write_text('old entry')
        with redirect_stdout(io.StringIO()) as said:
            backup=install.back_up(self.dest,[Path('starmeter.30s.sh'),Path('.opencodex-panel/lib')],root)
        self.assertEqual(said.getvalue(),'Removed 4 older backups\n')
        self.assertEqual((backup/'starmeter.30s.sh').read_text(),'old entry');self.assertEqual((backup/'.opencodex-panel/lib/old.py').read_text(),'old')
        # The new backup stays even when the clock once ran ahead; four others stay with it.
        kept=sorted(p.name for p in root.iterdir() if p.is_dir() and not p.is_symlink() and install.BACKUP_NAME.fullmatch(p.name))
        self.assertEqual(kept,sorted(old[4:]+[backup.name]))
        self.assertTrue(all((root/name).is_dir() for name in others));self.assertTrue((root/'20261101-090000-aaaaaa.zip').is_file())
        self.assertTrue((root/'20261101-080000-bbbbbb').is_symlink());self.assertEqual((outside/'keep').read_text(),'kept')
    def test_a_failed_backup_prunes_nothing_and_a_failed_prune_installs_anyway(self):
        old=['20261101-10000%d-abcde%d'%(i,i) for i in range(7)];root=self.backups(old)
        with patch.object(install,'copy',side_effect=OSError('disk full')),self.assertRaises(OSError):
            install.back_up(self.dest,[Path('.opencodex-panel/lib')],root)
        self.assertTrue(all((root/name).is_dir() for name in old))
        with patch.object(install.shutil,'rmtree',side_effect=OSError('busy')):
            backup=install.back_up(self.dest,[Path('.opencodex-panel/lib')],root)
        self.assertTrue((backup/'.opencodex-panel/lib/old.py').is_file());self.assertTrue(all((root/name).is_dir() for name in old))


LEGACY_CONFIG='export KOMARI_BASE_URL=https://monitor.invalid\n'


class ConfigMigrationTests(unittest.TestCase):
    """The plugin folder's old config moves into the state folder once. The installer and the Python reference each
    carry a copy of migrate_config (a release package ships no lib/ code); both must behave the same."""
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
    def fresh(self,name,target_text=None):
        root=Path(self.temp.name)/name;self.dest=root/'plugins';self.state=root/'state'
        self.legacy=self.dest/'.opencodex-panel/config.local.sh';self.legacy.parent.mkdir(parents=True);self.legacy.write_text(LEGACY_CONFIG)
        self.target=self.state/'config.local.sh'
        if target_text is not None:self.state.mkdir();self.target.write_text(target_text)
    def scratch_files(self):
        return sorted(p.name for p in self.state.iterdir() if p.name!='config.local.sh') if self.state.exists() else []
    def assert_migrated(self):
        self.assertEqual(self.target.read_text(),LEGACY_CONFIG);self.assertEqual(self.target.stat().st_mode&0o777,0o600)
        self.assertFalse(self.legacy.exists());self.assertEqual(self.scratch_files(),[])
    def assert_untouched(self):
        """A failed migration leaves the legacy file and no target at all, not even a partial or scratch one."""
        self.assertEqual(self.legacy.read_text(),LEGACY_CONFIG);self.assertFalse(self.target.exists());self.assertEqual(self.scratch_files(),[])
    def each(self,check):
        for name,module in (('installer',install),('reference',swiftbar_config)):
            with self.subTest(name):
                self.fresh(name);check(module)
    def test_the_legacy_config_moves_into_the_state_folder_once(self):
        def check(module):
            module.migrate_config(self.dest,self.state);self.assert_migrated()
            # An explicit import never replaces the file that is now there.
            with self.assertRaises(ValueError):module.migrate_config(self.dest,self.state,self.target)
            self.assertEqual(self.target.read_text(),LEGACY_CONFIG)
        self.each(check)
    def test_a_read_that_fails_leaves_nothing_and_the_next_install_migrates(self):
        # The report: a read error while copying left a zero-byte target, and the reinstall kept it as the user's file.
        def check(module):
            real=Path.read_bytes
            def read_bytes(path):
                if path==self.legacy:raise OSError('synthetic input read failure')
                return real(path)
            with patch.object(Path,'read_bytes',read_bytes),self.assertRaises(OSError):module.migrate_config(self.dest,self.state)
            self.assert_untouched()
            module.migrate_config(self.dest,self.state);self.assert_migrated()
        self.each(check)
    def test_a_write_that_fails_leaves_nothing_and_the_next_install_migrates(self):
        def check(module):
            with patch.object(install.os,'fsync',side_effect=OSError('disk full')),self.assertRaises(OSError):module.migrate_config(self.dest,self.state)
            self.assert_untouched()
            module.migrate_config(self.dest,self.state);self.assert_migrated()
        self.each(check)
    def test_an_existing_user_file_wins_even_when_it_is_empty(self):
        # A file the user emptied on purpose can't be told from one a failure left; both stay, the legacy file goes to the backup.
        for name,module in (('installer',install),('reference',swiftbar_config)):
            with self.subTest(name):
                for text in ('# examples only\n',''):
                    self.fresh(name+str(len(text)),text);module.migrate_config(self.dest,self.state)
                    self.assertEqual(self.target.read_text(),text);self.assertFalse(self.legacy.exists());self.assertEqual(self.scratch_files(),[])
                    (self.dest/'given.sh').write_text('# given\n')
                    with self.assertRaises(ValueError):module.migrate_config(self.dest,self.state,self.dest/'given.sh')
                    self.assertEqual(self.target.read_text(),text)
    def test_a_file_created_while_the_copy_was_written_wins(self):
        def check(module):
            self.state.mkdir()
            real=install.os.chmod
            def chmod(path,mode):
                real(path,mode);self.target.write_text('# saved from the panel meanwhile\n')
            with patch.object(install.os,'chmod',chmod):module.migrate_config(self.dest,self.state)
            self.assertEqual(self.target.read_text(),'# saved from the panel meanwhile\n');self.assertFalse(self.legacy.exists());self.assertEqual(self.scratch_files(),[])
        self.each(check)
    def test_a_volume_without_hard_links_still_publishes_whole(self):
        def check(module):
            with patch.object(install.os,'link',side_effect=PermissionError('no hard links')):module.migrate_config(self.dest,self.state)
            self.assert_migrated()
        self.each(check)
    NO_LINKS=OSError(errno.ENOTSUP,'no hard links on this volume')
    def test_a_file_created_just_before_the_fallback_rename_wins(self):
        # The reopened report: without hard links, a file another writer created after the target check and before the
        # rename was replaced. The exclusive rename refuses it however late it appears: here, right before the call.
        def check(module):
            real=module.rename_exclusive
            def rename_exclusive(source,target):
                Path(target).write_text('# saved from the panel meanwhile\n');real(source,target)
            with patch.object(install.os,'link',side_effect=self.NO_LINKS),patch.object(module,'rename_exclusive',rename_exclusive):
                module.migrate_config(self.dest,self.state)
            self.assertEqual(self.target.read_text(),'# saved from the panel meanwhile\n');self.assertFalse(self.legacy.exists());self.assertEqual(self.scratch_files(),[])
        self.each(check)
    def test_the_exclusive_rename_moves_a_file_or_refuses_an_existing_one(self):
        def check(module):
            self.state.mkdir();source=self.state/'source';taken=self.state/'taken';source.write_text('copy')
            module.rename_exclusive(source,self.target)
            self.assertEqual(self.target.read_text(),'copy');self.assertFalse(source.exists())
            source.write_text('another copy');taken.write_text('theirs')
            with self.assertRaises(FileExistsError):module.rename_exclusive(source,taken)
            self.assertEqual(taken.read_text(),'theirs');self.assertEqual(source.read_text(),'another copy')
        self.each(check)
    def test_a_volume_with_neither_hard_links_nor_an_exclusive_rename_keeps_the_legacy_file(self):
        # Nothing to publish with exclusively: the copy is dropped, the legacy file stays and the error says a reinstall migrates again.
        def check(module):
            with patch.object(install.os,'link',side_effect=self.NO_LINKS),\
                 patch.object(module,'rename_exclusive',side_effect=OSError(errno.ENOTSUP,'RENAME_EXCL unsupported')),\
                 self.assertRaises(OSError) as refused:
                module.migrate_config(self.dest,self.state)
            self.assertIn('重新安装',str(refused.exception));self.assertNotIsInstance(refused.exception,FileExistsError)
            self.assert_untouched()
            module.migrate_config(self.dest,self.state);self.assert_migrated()
        self.each(check)


if __name__=='__main__':
    unittest.main()
