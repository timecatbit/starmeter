import io
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import homebrew_cask


class Response(io.BytesIO):
    def __enter__(self):return self
    def __exit__(self, *_):self.close()


class CaskTests(unittest.TestCase):
    ARM, INTEL = 'a' * 64, 'b' * 64

    def test_the_cask_points_at_the_published_disk_images(self):
        text = homebrew_cask.cask('6.0.0', self.ARM, self.INTEL)
        self.assertIn('version "6.0.0"', text); self.assertIn('app "Starmeter.app"', text)
        # It updates itself, so brew upgrade leaves it to that.
        self.assertIn('auto_updates true', text)
        url = text.split('url "')[1].split('"')[0]
        for arch, digest in (('apple-silicon', self.ARM), ('intel', self.INTEL)):
            resolved = url.replace('#{version}', '6.0.0').replace('#{arch}', arch)
            self.assertTrue(resolved.startswith('https://github.com/timecatbit/starmeter/releases/download/v6.0.0/'))
            self.assertIn(resolved.rsplit('/', 1)[1], [name for name, _ in homebrew_cask.downloads('6.0.0')])
            self.assertIn(digest, text)
        with self.assertRaises(ValueError):homebrew_cask.cask('6.0.0', 'nope', self.INTEL)

    def test_digests_come_from_the_release(self):
        assets = [{'name': 'starmeter-6.0.0-macos-apple-silicon.dmg', 'digest': 'sha256:' + self.ARM},
                  {'name': 'starmeter-6.0.0-macos-intel.dmg', 'digest': 'sha256:' + self.INTEL}]
        seen = []
        def opener(request, timeout):
            seen.append(request.full_url);return Response(json.dumps({'assets': assets}).encode())
        self.assertEqual(homebrew_cask.published('6.0.0', opener), [self.ARM, self.INTEL])
        self.assertTrue(seen[0].endswith('/releases/tags/v6.0.0'))
        with self.assertRaises(SystemExit):homebrew_cask.published('6.0.0', lambda r, timeout: Response(b'{"assets":[]}'))


class FormulaTests(unittest.TestCase):
    MAC, LINUX = 'c' * 64, 'd' * 64

    def test_the_formula_installs_the_published_command(self):
        text = homebrew_cask.formula('6.1.0', self.MAC, self.LINUX)
        self.assertTrue(text.startswith('class StarmeterCli < Formula'))
        self.assertIn('version "6.1.0"', text)
        published = [name for name, _ in homebrew_cask.downloads('6.1.0')]
        urls = [line.split('"')[1] for line in text.splitlines() if line.strip().startswith('url "https://github.com/timecatbit/starmeter/releases')]
        self.assertEqual([url.rsplit('/', 1)[1] for url in urls], ['starmeter-6.1.0-macos-cli.tar.gz', 'starmeter-6.1.0-linux-x64-cli.tar.gz'])
        for url in urls:
            self.assertTrue(url.startswith('https://github.com/timecatbit/starmeter/releases/download/v6.1.0/'))
            self.assertIn(url.rsplit('/', 1)[1], published)
        self.assertLess(text.index(self.MAC), text.index('on_linux'))
        self.assertGreater(text.index(self.LINUX), text.index('on_linux'))
        self.assertIn('bin.install_symlink libexec/"starmeter"', text)
        self.assertIn('assert_match "starmeter #{version}"', text)
        with self.assertRaises(ValueError):homebrew_cask.formula('6.1.0', self.MAC, 'nope')

    def test_the_archives_digests_come_from_the_release(self):
        assets = [{'name': 'starmeter-6.1.0-linux-x64-cli.tar.gz', 'digest': 'sha256:' + self.LINUX},
                  {'name': 'starmeter-6.1.0-macos-cli.tar.gz', 'digest': 'sha256:' + self.MAC}]
        opener = lambda request, timeout: Response(json.dumps({'assets': assets}).encode())
        self.assertEqual(homebrew_cask.published('6.1.0', opener, suffixes=('-cli.tar.gz',)), [self.LINUX, self.MAC])


if __name__ == '__main__':
    unittest.main(verbosity=2)
