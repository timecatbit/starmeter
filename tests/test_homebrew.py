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


if __name__ == '__main__':
    unittest.main(verbosity=2)
