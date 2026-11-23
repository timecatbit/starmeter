"""The named API keys' store: a key's file and the index that lists it go together."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import swiftbar_keys as keys


class KeyStoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.state = Path(self.temp.name)

    def files(self, provider):
        folder = keys.root(self.state) / provider
        return len(list(folder.glob('*.key'))) if folder.is_dir() else 0

    def test_a_key_whose_index_cant_be_saved_leaves_no_file_behind(self):
        # SECRET-1: the index unwritable (a folder where it goes), the key's file is taken back; added again once it
        # can be, one file and one entry. A bad name writes nothing at all.
        (keys.root(self.state) / 'index.json').mkdir(parents=True)
        value = 'sk-' + 'a' * 24
        with self.assertRaises(OSError):keys.add(self.state, 'kimi-api', '工作', value)
        self.assertEqual((self.files('kimi-api'), len(keys.entries(self.state, 'kimi-api'))), (0, 0))
        (keys.root(self.state) / 'index.json').rmdir()
        with self.assertRaises(ValueError):keys.add(self.state, 'kimi-api', '', value)
        self.assertEqual(self.files('kimi-api'), 0)
        keys.add(self.state, 'kimi-api', '工作', value)
        self.assertEqual((self.files('kimi-api'), len(keys.entries(self.state, 'kimi-api'))), (1, 1))


if __name__ == '__main__':
    unittest.main()
